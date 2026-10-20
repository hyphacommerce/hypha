use hypha_apps::{App, StageApps};
use hypha_auth::{
    Identity, PasswordReset, Session, StageIdentities, StagePasswordResets, StageSessions,
    keys as auth_keys,
};
use hypha_cart::{Cart, StageCarts, repo::keys as cart_keys};
use hypha_categories::{Category, StageCategories, keys as category_keys};
use hypha_channels::StagePublications;
use hypha_core::AppKey;
use hypha_core::{
    CatalogId, CategoryId, ChannelId, HasBatch, IdempotencyRecord, IdentityId, PendingEvent, Price,
    PriceListId, ProductId, Recorded, RepoError, SessionId, StageEvents, StageIdempotency,
    TaxClassId, Timestamp, VariantId, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_custom_fields::{
    CustomFieldDefinition, StageCustomFieldDefinitions, StageCustomFieldValues, ValueRow,
    keys as custom_field_keys,
};
use hypha_customers::{Customer, StageCustomers, repo::keys as customer_keys};
use hypha_fulfillment::{Fulfillment, StageFulfillments, keys as fulfillment_keys};
use hypha_inventory::{InventoryItem, StageInventoryItems};
use hypha_markets::{StageCatalogProducts, StageFixedPrices};
use hypha_media::{StageAssetUsages, UsageOwner, UsageRole};
use hypha_notifications::{
    NotificationSettings, StageNotificationSettings, keys as notification_keys,
};
use hypha_oauth::{
    AuthorizationCode, AuthorizationRequest, OAuthToken, StageOAuth, keys as oauth_keys,
};
use hypha_orders::{Order, StageOrders, repo::keys as order_keys};
use hypha_payments::{Payment, StagePayments, keys as payment_keys};
use hypha_products::{Product, StageProducts};
use hypha_tax::StageTaxAssignments;
use hypha_users::{Invite, Role, StageInvites, StageRoles, StageUsers, User, keys as user_keys};

use crate::{
    InMemoryPorts,
    auth::{keep_recorded_use, unique_identity, unique_reset, unique_session},
    categories::{check_parent, unique as unique_category},
    customers::unique as unique_customer,
    idempotency::insert_record,
    inventory_items,
    media::replace_usages,
    orders::unique as unique_order,
    products::unique as unique_product,
    table::{insert_row, missing, no_unique_keys, update_row},
    users::{unique_invite, unique_role, unique_user},
};

/// Writes staged for `InMemoryPorts::commit`, in the order they were staged: copies
/// of the aggregates as they were staged, still carrying the version they were loaded
/// at. Mirrors `PgBatch`'s list of staged writes (design note 0016, EV2).
pub struct InMemoryBatch {
    staged: Vec<Staged>,
    /// When the batch was made: every aggregate it stages is stamped with this.
    now: Timestamp,
}

enum Staged {
    NewOrder(Order),
    Order(Order),
    NewFulfillment(Fulfillment),
    Fulfillment(Fulfillment),
    NewPayment(Payment),
    Payment(Payment),
    NewCart(Cart),
    Cart(Cart),
    Product(Product),
    /// A product removed at the version it was loaded at.
    DeletedProduct(Product),
    AssetUsages(UsageOwner, Vec<(hypha_core::AssetId, UsageRole)>),
    InventoryItem(InventoryItem),
    DeletedInventoryItem(InventoryItem),
    NewCustomer(Customer),
    Idempotency(IdempotencyRecord),
    NewRole(Role),
    Role(Role),
    NewUser(User),
    User(User),
    Invite(Invite),
    NewIdentity(Identity),
    Identity(Identity),
    NewSession(Session),
    Session(Session),
    SessionRevocation(IdentityId, Option<SessionId>, Timestamp),
    NewPasswordReset(PasswordReset),
    PasswordReset(PasswordReset),
    UsedUpResets(IdentityId, Timestamp),
    CatalogProducts(CatalogId, Vec<ProductId>, bool),
    Publications(ChannelId, Vec<ProductId>, bool),
    NewCategory(Category),
    Category(Category),
    DeletedCategory(CategoryId),
    CategoryProducts(CategoryId, Vec<ProductId>, bool),
    TaxAssignment(Vec<ProductId>, Option<TaxClassId>),
    FixedPrices(PriceListId, Vec<(VariantId, Option<Price>)>),
    NewNotificationSettings(NotificationSettings),
    NotificationSettings(NotificationSettings),
    NewApp(App),
    App(App),
    NewOAuthRequest(AuthorizationRequest),
    OAuthRequest(AuthorizationRequest),
    NewOAuthCode(AuthorizationCode),
    OAuthCode(AuthorizationCode),
    NewOAuthToken(OAuthToken),
    OAuthToken(OAuthToken),
    RevokeAppTokens(AppKey, Timestamp),
    NewCustomFieldDefinition(CustomFieldDefinition),
    CustomFieldDefinition(CustomFieldDefinition),
    DeletedCustomFieldDefinition(CustomFieldDefinition),
    CustomFieldValue(ValueRow, Option<Version>),
    RemovedCustomFieldValue(
        hypha_core::CustomFieldDefinitionId,
        uuid::Uuid,
        Option<Version>,
    ),
    Events(Vec<PendingEvent>, Option<Version>),
}

impl StageApps for InMemoryBatch {
    fn insert_app(&mut self, app: &mut App) {
        stamp_insert(app, self.now);
        let events = app.take_events();
        self.stage(Staged::NewApp(app.clone()));
        if !events.is_empty() {
            self.stage(Staged::Events(events, Some(Version::NEW.next())));
        }
        app.set_version(Version::NEW.next());
    }

    fn update_app(&mut self, app: &mut App) {
        stamp_update(app, self.now);
        let version = app.version().next();
        let events = app.take_events();
        self.stage(Staged::App(app.clone()));
        if !events.is_empty() {
            self.stage(Staged::Events(events, Some(version)));
        }
        app.set_version(version);
    }
}

impl StageOAuth for InMemoryBatch {
    fn insert_oauth_request(&mut self, request: &mut AuthorizationRequest) {
        self.stage(Staged::NewOAuthRequest(request.clone()));
        request.set_version(Version::NEW.next());
    }

    fn update_oauth_request(&mut self, request: &mut AuthorizationRequest) {
        self.stage(Staged::OAuthRequest(request.clone()));
        request.set_version(request.version().next());
    }

    fn insert_oauth_code(&mut self, code: &mut AuthorizationCode) {
        self.stage(Staged::NewOAuthCode(code.clone()));
        code.set_version(Version::NEW.next());
    }

    fn update_oauth_code(&mut self, code: &mut AuthorizationCode) {
        self.stage(Staged::OAuthCode(code.clone()));
        code.set_version(code.version().next());
    }

    fn insert_oauth_token(&mut self, token: &mut OAuthToken) {
        self.stage(Staged::NewOAuthToken(token.clone()));
        token.set_version(Version::NEW.next());
    }

    fn update_oauth_token(&mut self, token: &mut OAuthToken) {
        self.stage(Staged::OAuthToken(token.clone()));
        token.set_version(token.version().next());
    }

    fn revoke_app_tokens(&mut self, app: &AppKey, at: Timestamp) {
        self.stage(Staged::RevokeAppTokens(app.clone(), at));
    }
}

impl InMemoryBatch {
    fn stage(&mut self, write: Staged) {
        self.staged.push(write);
    }

    /// Stages the events `aggregate` recorded, about the write staged with them, which
    /// gives it `version`.
    fn stage_events_of(&mut self, aggregate: &mut impl Recorded, version: Version) {
        let events = aggregate.take_events();
        if !events.is_empty() {
            self.stage(Staged::Events(events, Some(version)));
        }
    }
}

impl StageEvents for InMemoryBatch {
    fn record_events(&mut self, events: Vec<PendingEvent>) {
        if !events.is_empty() {
            self.stage(Staged::Events(events, None));
        }
    }
}

impl StageIdentities for InMemoryBatch {
    fn insert_identity(&mut self, identity: &mut Identity) {
        self.stage(Staged::NewIdentity(identity.clone()));
        identity.set_version(Version::NEW.next());
    }

    fn update_identity(&mut self, identity: &mut Identity) {
        self.stage(Staged::Identity(identity.clone()));
        identity.set_version(identity.version().next());
    }
}

impl StageSessions for InMemoryBatch {
    fn insert_session(&mut self, session: &mut Session) {
        self.stage(Staged::NewSession(session.clone()));
        session.set_version(Version::NEW.next());
    }

    fn update_session(&mut self, session: &mut Session) {
        self.stage(Staged::Session(session.clone()));
        session.set_version(session.version().next());
    }

    fn revoke_sessions(&mut self, identity: IdentityId, keep: Option<SessionId>, at: Timestamp) {
        self.stage(Staged::SessionRevocation(identity, keep, at));
    }
}

impl StagePasswordResets for InMemoryBatch {
    fn insert_password_reset(&mut self, reset: &mut PasswordReset) {
        self.stage(Staged::NewPasswordReset(reset.clone()));
        reset.set_version(Version::NEW.next());
    }

    fn update_password_reset(&mut self, reset: &mut PasswordReset) {
        self.stage(Staged::PasswordReset(reset.clone()));
        reset.set_version(reset.version().next());
    }

    fn use_up_password_resets(&mut self, identity: IdentityId, at: Timestamp) {
        self.stage(Staged::UsedUpResets(identity, at));
    }
}

impl StageRoles for InMemoryBatch {
    fn insert_role(&mut self, role: &mut Role) {
        self.stage(Staged::NewRole(role.clone()));
        role.set_version(Version::NEW.next());
    }

    fn update_role(&mut self, role: &mut Role) {
        self.stage(Staged::Role(role.clone()));
        role.set_version(role.version().next());
    }
}

impl StageUsers for InMemoryBatch {
    fn insert_user(&mut self, user: &mut User) {
        self.stage(Staged::NewUser(user.clone()));
        user.set_version(Version::NEW.next());
    }

    fn update_user(&mut self, user: &mut User) {
        self.stage(Staged::User(user.clone()));
        user.set_version(user.version().next());
    }
}

impl StageInvites for InMemoryBatch {
    fn update_invite(&mut self, invite: &mut Invite) {
        self.stage(Staged::Invite(invite.clone()));
        invite.set_version(invite.version().next());
    }
}

impl StageCarts for InMemoryBatch {
    fn insert_cart(&mut self, cart: &mut Cart) {
        stamp_insert(cart, self.now);
        self.stage(Staged::NewCart(cart.clone()));
        cart.set_version(Version::NEW.next());
    }

    fn update_cart(&mut self, cart: &mut Cart) {
        stamp_update(cart, self.now);
        self.stage(Staged::Cart(cart.clone()));
        cart.set_version(cart.version().next());
    }
}

impl StageProducts for InMemoryBatch {
    fn update_product(&mut self, product: &mut Product) {
        stamp_update(product, self.now);
        // Events first: the copy staged (and stored) must not carry them, or a later
        // load would hand them out again.
        let version = product.version().next();
        self.stage_events_of(product, version);
        self.stage(Staged::Product(product.clone()));
        product.set_version(version);
    }

    fn delete_product(&mut self, product: &mut Product) {
        let events = product.take_events();
        self.stage(Staged::DeletedProduct(product.clone()));
        self.stage(Staged::Events(events, None));
    }
}

impl StageAssetUsages for InMemoryBatch {
    fn replace_asset_usages(
        &mut self,
        owner: &UsageOwner,
        usages: &[(hypha_core::AssetId, UsageRole)],
    ) {
        self.stage(Staged::AssetUsages(owner.clone(), usages.to_vec()));
    }
}

impl StageInventoryItems for InMemoryBatch {
    fn update_inventory_item(&mut self, item: &mut InventoryItem) {
        stamp_update(item, self.now);
        let version = item.version().next();
        self.stage_events_of(item, version);
        self.stage(Staged::InventoryItem(item.clone()));
        item.set_version(version);
    }

    fn delete_inventory_item(&mut self, item: &mut InventoryItem) {
        let events = item.take_events();
        self.stage(Staged::DeletedInventoryItem(item.clone()));
        self.stage(Staged::Events(events, None));
    }
}

impl StageOrders for InMemoryBatch {
    fn insert_order(&mut self, order: &mut Order) {
        stamp_insert(order, self.now);
        // The copy staged keeps no events: they're staged on their own, next.
        let events = order.take_events();
        self.stage(Staged::NewOrder(order.clone()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        order.set_version(Version::NEW.next());
    }

    fn update_order(&mut self, order: &mut Order) {
        stamp_update(order, self.now);
        let version = order.version().next();
        self.stage_events_of(order, version);
        self.stage(Staged::Order(order.clone()));
        order.set_version(version);
    }
}

impl StageFulfillments for InMemoryBatch {
    fn insert_fulfillment(&mut self, fulfillment: &mut Fulfillment) {
        stamp_insert(fulfillment, self.now);
        let events = fulfillment.take_events();
        self.stage(Staged::NewFulfillment(fulfillment.clone()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        fulfillment.set_version(Version::NEW.next());
    }

    fn update_fulfillment(&mut self, fulfillment: &mut Fulfillment) {
        stamp_update(fulfillment, self.now);
        let version = fulfillment.version().next();
        self.stage_events_of(fulfillment, version);
        self.stage(Staged::Fulfillment(fulfillment.clone()));
        fulfillment.set_version(version);
    }
}

impl StagePayments for InMemoryBatch {
    fn insert_payment(&mut self, payment: &mut Payment) {
        stamp_insert(payment, self.now);
        let events = payment.take_events();
        self.stage(Staged::NewPayment(payment.clone()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        payment.set_version(Version::NEW.next());
    }

    fn update_payment(&mut self, payment: &mut Payment) {
        stamp_update(payment, self.now);
        let version = payment.version().next();
        self.stage_events_of(payment, version);
        self.stage(Staged::Payment(payment.clone()));
        payment.set_version(version);
    }
}

impl StageCustomers for InMemoryBatch {
    fn insert_customer(&mut self, customer: &mut Customer) {
        stamp_insert(customer, self.now);
        let events = customer.take_events();
        self.stage(Staged::NewCustomer(customer.clone()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        customer.set_version(Version::NEW.next());
    }
}

impl StageIdempotency for InMemoryBatch {
    fn record_idempotency(&mut self, record: IdempotencyRecord) {
        self.stage(Staged::Idempotency(record));
    }
}

impl HasBatch for InMemoryPorts {
    type Batch = InMemoryBatch;

    fn batch(&self) -> InMemoryBatch {
        InMemoryBatch {
            staged: Vec::new(),
            now: self.outbox.now(),
        }
    }

    /// Locks every table a batch can write, always in this order, so two commits can't
    /// deadlock and a single write (which locks one table) can't slip in between. The
    /// writes then run with the usual version and key checks; if one fails, the tables
    /// are put back as they were. Readers wait for the locks, so they never see half a
    /// batch. Copying the tables for the rollback is fine at test sizes.
    async fn commit(&self, batch: InMemoryBatch) -> Result<(), RepoError> {
        let mut orders = self.orders.orders.lock();
        let mut fulfillments = self.fulfillments.fulfillments.lock();
        let fulfillments_before = fulfillments.clone();
        let mut payments = self.payments.payments.lock();
        let payments_before = payments.clone();
        let mut carts = self.carts.carts.lock();
        let mut products = self.products.products.lock();
        let mut items = self.inventory_items.items.lock();
        let mut customers = self.customers.customers.lock();
        let mut records = self.idempotency.lock();
        let mut identities = self.identities.identities.lock();
        let mut sessions = self.sessions.sessions.lock();
        let mut resets = self.password_resets.resets.lock();
        let mut roles = self.roles.roles.lock();
        let mut users = self.users.users.lock();
        let mut invites = self.invites.invites.lock();
        let assets = self.assets.assets.lock();
        let mut usages = self.assets.lock_usages();
        let usages_before = usages.clone();
        let mut memberships = self.catalogs.lock_products();
        let memberships_before = memberships.clone();
        let mut publications = self.channels.lock_products();
        let publications_before = publications.clone();
        let mut categories = self.categories.categories.lock();
        let categories_before = categories.clone();
        let mut category_members = self.categories.lock_members();
        let category_members_before = category_members.clone();
        let mut fixed_prices = self.price_lists.lock_fixed_prices();
        let fixed_prices_before = fixed_prices.clone();
        let mut tax_classes = self.tax_classes.lock_assignments();
        let tax_classes_before = tax_classes.clone();
        let mut notification_settings = self.notification_settings.settings.lock();
        let notification_settings_before = notification_settings.clone();
        let mut apps = self.apps.apps.lock().unwrap();
        let apps_before = apps.clone();
        let mut oauth = self.oauth.lock();
        let oauth_before = oauth.clone();
        let mut field_definitions = self.custom_fields.definitions.lock();
        let field_definitions_before = field_definitions.clone();
        let mut field_values = self.custom_fields.lock_values();
        let field_values_before = field_values.clone();
        let now = self.outbox.now();
        // Single writes first, the set-based revocation after them (as Postgres sorts).
        let mut revocations = Vec::new();
        let before = (
            orders.clone(),
            carts.clone(),
            products.clone(),
            items.clone(),
            customers.clone(),
            records.clone(),
        );
        let before_staff = (
            identities.clone(),
            sessions.clone(),
            resets.clone(),
            roles.clone(),
            users.clone(),
            invites.clone(),
        );

        // Written only if everything else is: an event exists iff its change committed.
        let mut events = Vec::new();
        let written = (|| {
            for write in batch.staged {
                match write {
                    Staged::NewOrder(mut order) => insert_row(
                        &mut orders,
                        order.id(),
                        &mut order,
                        order_keys::ORDER_ID,
                        &unique_order,
                    )?,
                    Staged::Order(mut order) => {
                        update_row(&mut orders, order.id(), &mut order, &unique_order)?
                    }
                    Staged::NewFulfillment(mut fulfillment) => insert_row(
                        &mut fulfillments,
                        fulfillment.id(),
                        &mut fulfillment,
                        fulfillment_keys::FULFILLMENT_ID,
                        &no_unique_keys,
                    )?,
                    Staged::Fulfillment(mut fulfillment) => update_row(
                        &mut fulfillments,
                        fulfillment.id(),
                        &mut fulfillment,
                        &no_unique_keys,
                    )?,
                    Staged::NewPayment(mut payment) => insert_row(
                        &mut payments,
                        payment.id(),
                        &mut payment,
                        payment_keys::PAYMENT_ID,
                        &no_unique_keys,
                    )?,
                    Staged::Payment(mut payment) => {
                        update_row(&mut payments, payment.id(), &mut payment, &no_unique_keys)?
                    }
                    Staged::NewCart(mut cart) => insert_row(
                        &mut carts,
                        cart.id(),
                        &mut cart,
                        cart_keys::CART_ID,
                        &no_unique_keys,
                    )?,
                    Staged::Cart(mut cart) => {
                        update_row(&mut carts, cart.id(), &mut cart, &no_unique_keys)?
                    }
                    Staged::Product(mut product) => {
                        update_row(&mut products, product.id(), &mut product, &unique_product)?
                    }
                    Staged::DeletedProduct(product) => {
                        match products.get(&product.id()) {
                            Some(stored) if stored.version() == product.version() => {}
                            _ => return Err(RepoError::StaleVersion),
                        }
                        let id = product.id();
                        products.remove(&id);
                        // What Postgres removes with the row: what only names it.
                        for included in memberships.values_mut() {
                            included.remove(&id);
                        }
                        for published in publications.values_mut() {
                            published.remove(&id);
                        }
                        for members in category_members.values_mut() {
                            members.remove(&id);
                        }
                        tax_classes.remove(&id);
                    }
                    Staged::AssetUsages(owner, replacement) => {
                        replace_usages(&assets, &mut usages, &owner, &replacement)?
                    }
                    Staged::InventoryItem(mut item) => {
                        update_row(&mut items, item.id(), &mut item, &inventory_items::unique)?
                    }
                    Staged::DeletedInventoryItem(item) => {
                        match items.get(&item.id()) {
                            Some(stored) if stored.version() == item.version() => {}
                            _ => return Err(RepoError::StaleVersion),
                        }
                        items.remove(&item.id());
                    }
                    Staged::NewCustomer(mut customer) => insert_row(
                        &mut customers,
                        customer.id(),
                        &mut customer,
                        customer_keys::CUSTOMER_ID,
                        &unique_customer,
                    )?,
                    Staged::Idempotency(record) => insert_record(&mut records, record)?,
                    Staged::NewRole(mut role) => insert_row(
                        &mut roles,
                        role.id(),
                        &mut role,
                        user_keys::ROLE_ID,
                        &unique_role,
                    )?,
                    Staged::Role(mut role) => {
                        update_row(&mut roles, role.id(), &mut role, &unique_role)?
                    }
                    Staged::NewUser(mut user) => insert_row(
                        &mut users,
                        user.id(),
                        &mut user,
                        user_keys::USER_ID,
                        &unique_user,
                    )?,
                    Staged::User(mut user) => {
                        update_row(&mut users, user.id(), &mut user, &unique_user)?
                    }
                    Staged::Invite(mut invite) => {
                        update_row(&mut invites, invite.id(), &mut invite, &unique_invite)?
                    }
                    Staged::NewIdentity(mut identity) => insert_row(
                        &mut identities,
                        identity.id(),
                        &mut identity,
                        auth_keys::IDENTITY_ID,
                        &unique_identity,
                    )?,
                    Staged::Identity(mut identity) => update_row(
                        &mut identities,
                        identity.id(),
                        &mut identity,
                        &unique_identity,
                    )?,
                    Staged::NewSession(mut session) => insert_row(
                        &mut sessions,
                        session.id(),
                        &mut session,
                        auth_keys::SESSION_ID,
                        &unique_session,
                    )?,
                    Staged::Session(mut session) => {
                        keep_recorded_use(&sessions, &mut session);
                        update_row(&mut sessions, session.id(), &mut session, &unique_session)?
                    }
                    Staged::SessionRevocation(identity, keep, at) => {
                        for session in sessions.values_mut().filter(|s| {
                            s.identity() == identity
                                && Some(s.id()) != keep
                                && s.revoked_at().is_none()
                        }) {
                            session.revoke(at);
                            session.set_version(session.version().next());
                        }
                    }
                    Staged::NewPasswordReset(mut reset) => insert_row(
                        &mut resets,
                        reset.id(),
                        &mut reset,
                        auth_keys::PASSWORD_RESET_ID,
                        &unique_reset,
                    )?,
                    Staged::PasswordReset(mut reset) => {
                        update_row(&mut resets, reset.id(), &mut reset, &unique_reset)?
                    }
                    Staged::UsedUpResets(identity, at) => {
                        for reset in resets
                            .values_mut()
                            .filter(|r| r.identity() == identity && r.used_at().is_none())
                        {
                            reset.mark_used(at);
                            reset.set_version(reset.version().next());
                        }
                    }
                    Staged::CatalogProducts(catalog, products, add) => {
                        if !self.catalogs.exists(catalog) {
                            return Err(missing("catalog", catalog));
                        }
                        let included = memberships.entry(catalog).or_default();
                        for product in products {
                            if add {
                                included.insert(product);
                            } else {
                                included.remove(&product);
                            }
                        }
                    }
                    Staged::Publications(channel, products, publish) => {
                        if !self.channels.exists(channel) {
                            return Err(missing("channel", channel));
                        }
                        let published = publications.entry(channel).or_default();
                        for product in products {
                            if publish {
                                published.insert(product);
                            } else {
                                published.remove(&product);
                            }
                        }
                    }
                    Staged::NewCategory(mut category) => {
                        check_parent(&categories, &category)?;
                        insert_row(
                            &mut categories,
                            category.id(),
                            &mut category,
                            category_keys::CATEGORY_ID,
                            &unique_category,
                        )?
                    }
                    Staged::Category(mut category) => {
                        check_parent(&categories, &category)?;
                        update_row(
                            &mut categories,
                            category.id(),
                            &mut category,
                            &unique_category,
                        )?
                    }
                    Staged::DeletedCategory(id) => {
                        // What the parent's foreign key enforces: no orphaned children.
                        if categories.values().any(|c| c.parent() == Some(id)) {
                            return Err(RepoError::Backend(format!("category {id} has children")));
                        }
                        categories.remove(&id);
                        category_members.remove(&id);
                    }
                    Staged::CategoryProducts(category, products, add) => {
                        if !categories.contains_key(&category) {
                            return Err(missing("category", category));
                        }
                        let members = category_members.entry(category).or_default();
                        for product in products {
                            if add {
                                members.insert(product);
                            } else {
                                members.remove(&product);
                            }
                        }
                    }
                    Staged::TaxAssignment(products, class) => {
                        for product in products {
                            match class {
                                Some(class) => tax_classes.insert(product, class),
                                None => tax_classes.remove(&product),
                            };
                        }
                    }
                    Staged::FixedPrices(list, changes) => {
                        if !self.price_lists.exists(list) {
                            return Err(missing("price list", list));
                        }
                        let prices = fixed_prices.entry(list).or_default();
                        for (variant, price) in changes {
                            match price {
                                Some(price) => prices.insert(variant, price),
                                None => prices.remove(&variant),
                            };
                        }
                    }
                    Staged::NewNotificationSettings(mut settings) => insert_row(
                        &mut notification_settings,
                        (),
                        &mut settings,
                        notification_keys::SETTINGS,
                        &no_unique_keys,
                    )?,
                    Staged::NotificationSettings(mut settings) => update_row(
                        &mut notification_settings,
                        (),
                        &mut settings,
                        &no_unique_keys,
                    )?,
                    Staged::NewApp(mut app) => crate::apps::insert_app(&mut apps, &mut app)?,
                    Staged::App(mut app) => crate::apps::update_app(&mut apps, &mut app)?,
                    Staged::NewOAuthRequest(mut request) => insert_row(
                        &mut oauth.requests,
                        request.id(),
                        &mut request,
                        oauth_keys::OAUTH_REQUEST_ID,
                        &no_unique_keys,
                    )?,
                    Staged::OAuthRequest(mut request) => update_row(
                        &mut oauth.requests,
                        request.id(),
                        &mut request,
                        &no_unique_keys,
                    )?,
                    Staged::NewOAuthCode(mut code) => {
                        crate::oauth::insert_code(&mut oauth, &mut code)?
                    }
                    Staged::OAuthCode(mut code) => {
                        update_row(&mut oauth.codes, code.hash(), &mut code, &no_unique_keys)?
                    }
                    Staged::NewOAuthToken(mut token) => {
                        if !apps.contains_key(token.app().as_str()) {
                            return Err(missing("app", token.app()));
                        }
                        insert_row(
                            &mut oauth.tokens,
                            token.id(),
                            &mut token,
                            oauth_keys::OAUTH_TOKEN_ID,
                            &crate::oauth::unique_token,
                        )?
                    }
                    Staged::OAuthToken(mut token) => update_row(
                        &mut oauth.tokens,
                        token.id(),
                        &mut token,
                        &crate::oauth::unique_token,
                    )?,
                    Staged::RevokeAppTokens(app, at) => revocations.push((app, at)),
                    Staged::NewCustomFieldDefinition(mut definition) => insert_row(
                        &mut field_definitions,
                        definition.id(),
                        &mut definition,
                        custom_field_keys::CUSTOM_FIELD_DEFINITION_ID,
                        &crate::custom_fields::unique_definition,
                    )?,
                    Staged::CustomFieldDefinition(mut definition) => update_row(
                        &mut field_definitions,
                        definition.id(),
                        &mut definition,
                        &crate::custom_fields::unique_definition,
                    )?,
                    Staged::DeletedCustomFieldDefinition(definition) => {
                        match field_definitions.get(&definition.id()) {
                            Some(stored) if stored.version() == definition.version() => {}
                            _ => return Err(RepoError::StaleVersion),
                        }
                        field_definitions.remove(&definition.id());
                        field_values.retain(|(d, _), _| *d != definition.id());
                    }
                    Staged::CustomFieldValue(row, expected) => crate::custom_fields::set_value(
                        &field_definitions,
                        &mut field_values,
                        row,
                        expected,
                        now,
                    )?,
                    Staged::RemovedCustomFieldValue(definition, owner, expected) => {
                        crate::custom_fields::remove_value(
                            &mut field_values,
                            definition,
                            owner,
                            expected,
                        )?
                    }
                    Staged::Events(staged, version) => events.push((staged, version)),
                }
            }
            for (app, at) in revocations {
                crate::oauth::revoke_where(&mut oauth, at, |t| t.app() == &app);
            }
            Ok(())
        })();
        if written.is_ok() {
            for (staged, version) in events {
                self.outbox.write(staged, version);
            }
        }
        if written.is_err() {
            (*orders, *carts, *products, *items, *customers, *records) = before;
            *usages = usages_before;
            *fulfillments = fulfillments_before;
            *payments = payments_before;
            *memberships = memberships_before;
            *publications = publications_before;
            *categories = categories_before;
            *category_members = category_members_before;
            *fixed_prices = fixed_prices_before;
            *tax_classes = tax_classes_before;
            *notification_settings = notification_settings_before;
            *apps = apps_before;
            *oauth = oauth_before;
            *field_definitions = field_definitions_before;
            *field_values = field_values_before;
            (*identities, *sessions, *resets, *roles, *users, *invites) = before_staff;
        }
        written
    }
}

impl StageNotificationSettings for InMemoryBatch {
    fn insert_notification_settings(&mut self, settings: &mut NotificationSettings) {
        stamp_insert(settings, self.now);
        self.stage(Staged::NewNotificationSettings(settings.clone()));
        settings.set_version(Version::NEW.next());
    }

    fn update_notification_settings(&mut self, settings: &mut NotificationSettings) {
        stamp_update(settings, self.now);
        self.stage(Staged::NotificationSettings(settings.clone()));
        settings.set_version(settings.version().next());
    }
}

impl StagePublications for InMemoryBatch {
    fn publish_products(&mut self, channel: ChannelId, products: &[ProductId]) {
        self.stage(Staged::Publications(channel, products.to_vec(), true));
    }

    fn unpublish_products(&mut self, channel: ChannelId, products: &[ProductId]) {
        self.stage(Staged::Publications(channel, products.to_vec(), false));
    }
}

impl StageCategories for InMemoryBatch {
    fn insert_category(&mut self, category: &mut Category) {
        stamp_insert(category, self.now);
        // The copy staged keeps no events: they're staged on their own, next.
        let events = category.take_events();
        self.stage(Staged::NewCategory(category.clone()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        category.set_version(Version::NEW.next());
    }

    fn update_category(&mut self, category: &mut Category) {
        stamp_update(category, self.now);
        let version = category.version().next();
        self.stage_events_of(category, version);
        self.stage(Staged::Category(category.clone()));
        category.set_version(version);
    }

    fn delete_category(&mut self, category: CategoryId) {
        self.stage(Staged::DeletedCategory(category));
    }

    fn add_category_products(&mut self, category: CategoryId, products: &[ProductId]) {
        self.stage(Staged::CategoryProducts(category, products.to_vec(), true));
    }

    fn remove_category_products(&mut self, category: CategoryId, products: &[ProductId]) {
        self.stage(Staged::CategoryProducts(category, products.to_vec(), false));
    }
}

impl StageCatalogProducts for InMemoryBatch {
    fn add_catalog_products(&mut self, catalog: CatalogId, products: &[ProductId]) {
        self.stage(Staged::CatalogProducts(catalog, products.to_vec(), true));
    }

    fn remove_catalog_products(&mut self, catalog: CatalogId, products: &[ProductId]) {
        self.stage(Staged::CatalogProducts(catalog, products.to_vec(), false));
    }
}

impl StageFixedPrices for InMemoryBatch {
    fn set_fixed_prices(&mut self, list: PriceListId, prices: &[(VariantId, Price)]) {
        let changes = prices.iter().map(|(v, p)| (*v, Some(*p))).collect();
        self.stage(Staged::FixedPrices(list, changes));
    }

    fn remove_fixed_prices(&mut self, list: PriceListId, variants: &[VariantId]) {
        let changes = variants.iter().map(|v| (*v, None)).collect();
        self.stage(Staged::FixedPrices(list, changes));
    }
}

impl StageTaxAssignments for InMemoryBatch {
    fn assign_tax_class(&mut self, products: &[ProductId], class: Option<TaxClassId>) {
        self.stage(Staged::TaxAssignment(products.to_vec(), class));
    }
}

impl StageCustomFieldDefinitions for InMemoryBatch {
    fn insert_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition) {
        stamp_insert(definition, self.now);
        let events = definition.take_events();
        self.stage(Staged::NewCustomFieldDefinition(definition.clone()));
        if !events.is_empty() {
            self.stage(Staged::Events(events, Some(Version::NEW.next())));
        }
        definition.set_version(Version::NEW.next());
    }

    fn update_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition) {
        stamp_update(definition, self.now);
        let version = definition.version().next();
        self.stage_events_of(definition, version);
        self.stage(Staged::CustomFieldDefinition(definition.clone()));
        definition.set_version(version);
    }

    fn delete_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition) {
        let events = definition.take_events();
        self.stage(Staged::DeletedCustomFieldDefinition(definition.clone()));
        if !events.is_empty() {
            self.stage(Staged::Events(events, None));
        }
    }
}

impl StageCustomFieldValues for InMemoryBatch {
    fn set_custom_field_value(&mut self, row: ValueRow, expected: Option<Version>) {
        self.stage(Staged::CustomFieldValue(row, expected));
    }

    fn remove_custom_field_value(
        &mut self,
        definition: hypha_core::CustomFieldDefinitionId,
        owner: uuid::Uuid,
        expected: Option<Version>,
    ) {
        self.stage(Staged::RemovedCustomFieldValue(definition, owner, expected));
    }
}
