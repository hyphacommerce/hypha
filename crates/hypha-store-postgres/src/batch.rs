use std::cmp::Ordering;

use hypha_auth::{
    Identity, IdentitySnapshot, PasswordReset, PasswordResetSnapshot, Session, SessionSnapshot,
    StageIdentities, StagePasswordResets, StageSessions,
};
use hypha_cart::{Cart, CartSnapshot, StageCarts};
use hypha_categories::{Category, CategorySnapshot, StageCategories};
use hypha_core::{
    CatalogId, CategoryId, ChannelId, HasBatch, IdempotencyRecord, IdentityId, PendingEvent, Price,
    PriceListId, ProductId, Recorded, RepoError, SessionId, StageEvents, StageIdempotency,
    TaxClassId, Timestamp, VariantId, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_customers::{Customer, CustomerSnapshot, StageCustomers};
use hypha_fulfillment::{Fulfillment, FulfillmentSnapshot, StageFulfillments};
use hypha_inventory::{InventoryItem, InventoryItemSnapshot, StageInventoryItems};
use hypha_markets::{StageCatalogProducts, StageFixedPrices};
use hypha_media::{StageAssetUsages, UsageOwner, UsageRole};
use hypha_orders::{Order, OrderSnapshot, StageOrders};
use hypha_products::{Product, ProductSnapshot, StageProducts};
use hypha_tax::StageTaxAssignments;
use hypha_users::{
    Invite, InviteSnapshot, Role, RoleSnapshot, StageInvites, StageRoles, StageUsers, User,
    UserSnapshot,
};

use crate::{
    auth::{
        insert_identity, insert_password_reset, insert_session, revoke_sessions, update_identity,
        update_password_reset, update_session, use_up_password_resets,
    },
    carts::{insert_cart, update_cart},
    categories::{
        add_category_products, delete_category, insert_category, remove_category_products,
        update_category,
    },
    customers::insert_customer,
    error::repo_error,
    fulfillments::{insert_fulfillment, update_fulfillment},
    idempotency::insert_records,
    inventory::update_items,
    inventory::{delete_items, lock_items},
    markets::{
        add_catalog_products, remove_catalog_products, remove_fixed_prices, set_fixed_prices,
    },
    media::replace_usages,
    orders::{insert_order, update_order},
    outbox::insert_events,
    products::update_product,
    store::{PgConnection, PgStore},
    tax::assign_tax_class,
    users::{insert_role, insert_user, update_invite, update_role, update_user},
};

/// Writes staged for `PgStore::commit`, in the order they were staged: snapshots taken
/// when each aggregate was staged, carrying the version it was loaded at.
pub struct PgBatch {
    staged: Vec<Staged>,
    /// When the batch was made: every aggregate it stages is stamped with this.
    now: Timestamp,
}

/// One staged write. A new aggregate a batch can write is one variant here, one arm in
/// `rank` and `apply`, and its `Stage*` impl below.
enum Staged {
    NewOrder(OrderSnapshot),
    Order(OrderSnapshot),
    NewFulfillment(FulfillmentSnapshot),
    Fulfillment(FulfillmentSnapshot),
    NewCart(CartSnapshot),
    Cart(CartSnapshot),
    Product(ProductSnapshot),
    AssetUsages(UsageOwner, Vec<(hypha_core::AssetId, UsageRole)>),
    NewCustomer(CustomerSnapshot),
    Idempotency(IdempotencyRecord),
    NewRole(RoleSnapshot),
    Role(RoleSnapshot),
    NewUser(UserSnapshot),
    User(UserSnapshot),
    Invite(InviteSnapshot),
    NewIdentity(IdentitySnapshot),
    Identity(IdentitySnapshot),
    NewSession(SessionSnapshot),
    Session(SessionSnapshot),
    SessionRevocation(IdentityId, Option<SessionId>, Timestamp),
    NewPasswordReset(PasswordResetSnapshot),
    PasswordReset(PasswordResetSnapshot),
    UsedUpResets(IdentityId, Timestamp),
    InventoryItem(InventoryItemSnapshot),
    /// An item removed at the version it was loaded at.
    DeletedInventoryItem(InventoryItemSnapshot),
    /// Products' tax class (`None`: the default rate).
    TaxAssignment(Vec<ProductId>, Option<TaxClassId>),
    /// Catalog membership: added (`true`) or removed.
    CatalogProducts(CatalogId, Vec<ProductId>, bool),
    /// Publications: published (`true`) or unpublished.
    Publications(ChannelId, Vec<ProductId>, bool),
    NewCategory(CategorySnapshot),
    Category(CategorySnapshot),
    DeletedCategory(CategoryId),
    /// Category membership: added (`true`) or removed.
    CategoryProducts(CategoryId, Vec<ProductId>, bool),
    /// Fixed prices set (`Some` price) or removed (`None`), per variant.
    FixedPrices(PriceListId, Vec<(VariantId, Option<Price>)>),
    /// Events about the aggregate staged just before, with the version that write gives
    /// it, or staged on their own (`StageEvents`, no version).
    Events(Vec<PendingEvent>, Option<Version>),
}

impl Staged {
    /// Where a write goes in the commit: tables in a fixed order, so two commits lock
    /// rows in the same order and queue instead of deadlocking. Inventory comes last: its
    /// rows are the most contended (every checkout of a popular item), so their locks
    /// are held for the shortest time. Idempotency records just before it: like items,
    /// they go in one statement however many there are.
    fn rank(&self) -> u8 {
        match self {
            Staged::NewOrder(_) => 0,
            Staged::Order(_) => 1,
            Staged::NewFulfillment(_) => 2,
            Staged::Fulfillment(_) => 3,
            Staged::NewCart(_) => 4,
            Staged::Cart(_) => 5,
            Staged::Product(_) => 6,
            Staged::AssetUsages(..) => 7,
            Staged::NewCustomer(_) => 8,
            Staged::NewRole(_) => 9,
            Staged::Role(_) => 10,
            Staged::NewUser(_) => 11,
            Staged::User(_) => 12,
            Staged::Invite(_) => 13,
            Staged::NewIdentity(_) => 14,
            Staged::Identity(_) => 15,
            Staged::NewSession(_) => 16,
            Staged::Session(_) => 17,
            Staged::SessionRevocation(..) => 18,
            Staged::NewPasswordReset(_) => 19,
            Staged::PasswordReset(_) => 20,
            Staged::UsedUpResets(..) => 21,
            // Categories before their membership (a new one must exist first).
            Staged::NewCategory(_) => 22,
            Staged::Category(_) => 23,
            Staged::DeletedCategory(_) => 24,
            Staged::CategoryProducts(..) => 25,
            // Set-based writes (memberships, fixed prices, tax classes): one statement
            // each, whatever the number of rows.
            Staged::CatalogProducts(..) => 26,
            Staged::Publications(..) => 27,
            Staged::FixedPrices(..) => 28,
            Staged::TaxAssignment(..) => 29,
            Staged::Idempotency(_) => 30,
            Staged::InventoryItem(_) => 31,
            Staged::DeletedInventoryItem(_) => 32,
            // Inserts into a table nothing else locks: anywhere would do.
            Staged::Events(..) => 33,
        }
    }

    /// Within a table, rows in key order (same reason). Writes without a row key keep
    /// the order they were staged in (the sort is stable).
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank()
            .cmp(&other.rank())
            .then_with(|| match (self, other) {
                (Staged::NewOrder(a), Staged::NewOrder(b))
                | (Staged::Order(a), Staged::Order(b)) => a.id.cmp(&b.id),
                (Staged::NewFulfillment(a), Staged::NewFulfillment(b))
                | (Staged::Fulfillment(a), Staged::Fulfillment(b)) => a.id.cmp(&b.id),
                (Staged::NewCart(a), Staged::NewCart(b)) | (Staged::Cart(a), Staged::Cart(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::Product(a), Staged::Product(b)) => a.id.cmp(&b.id),
                (Staged::AssetUsages(a, _), Staged::AssetUsages(b, _)) => a.cmp(b),
                (Staged::NewCustomer(a), Staged::NewCustomer(b)) => a.id.cmp(&b.id),
                (Staged::NewRole(a), Staged::NewRole(b)) | (Staged::Role(a), Staged::Role(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::NewUser(a), Staged::NewUser(b)) | (Staged::User(a), Staged::User(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::Invite(a), Staged::Invite(b)) => a.id.cmp(&b.id),
                (Staged::NewIdentity(a), Staged::NewIdentity(b))
                | (Staged::Identity(a), Staged::Identity(b)) => a.id.cmp(&b.id),
                (Staged::NewSession(a), Staged::NewSession(b))
                | (Staged::Session(a), Staged::Session(b)) => a.id.cmp(&b.id),
                (Staged::NewPasswordReset(a), Staged::NewPasswordReset(b))
                | (Staged::PasswordReset(a), Staged::PasswordReset(b)) => a.id.cmp(&b.id),
                (Staged::InventoryItem(a), Staged::InventoryItem(b))
                | (Staged::DeletedInventoryItem(a), Staged::DeletedInventoryItem(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::CatalogProducts(a, ..), Staged::CatalogProducts(b, ..)) => a.cmp(b),
                (Staged::Publications(a, ..), Staged::Publications(b, ..)) => a.cmp(b),
                (Staged::NewCategory(a), Staged::NewCategory(b))
                | (Staged::Category(a), Staged::Category(b)) => a.id.cmp(&b.id),
                (Staged::DeletedCategory(a), Staged::DeletedCategory(b)) => a.cmp(b),
                (Staged::CategoryProducts(a, ..), Staged::CategoryProducts(b, ..)) => a.cmp(b),
                (Staged::FixedPrices(a, _), Staged::FixedPrices(b, _)) => a.cmp(b),
                _ => Ordering::Equal,
            })
    }
}

impl PgBatch {
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

impl StageEvents for PgBatch {
    fn record_events(&mut self, events: Vec<PendingEvent>) {
        if !events.is_empty() {
            self.stage(Staged::Events(events, None));
        }
    }
}

impl StageRoles for PgBatch {
    fn insert_role(&mut self, role: &mut Role) {
        self.stage(Staged::NewRole(role.snapshot()));
        role.set_version(Version::NEW.next());
    }

    fn update_role(&mut self, role: &mut Role) {
        self.stage(Staged::Role(role.snapshot()));
        role.set_version(role.version().next());
    }
}

impl StageUsers for PgBatch {
    fn insert_user(&mut self, user: &mut User) {
        self.stage(Staged::NewUser(user.snapshot()));
        user.set_version(Version::NEW.next());
    }

    fn update_user(&mut self, user: &mut User) {
        self.stage(Staged::User(user.snapshot()));
        user.set_version(user.version().next());
    }
}

impl StageInvites for PgBatch {
    fn update_invite(&mut self, invite: &mut Invite) {
        self.stage(Staged::Invite(invite.snapshot()));
        invite.set_version(invite.version().next());
    }
}

impl StageIdentities for PgBatch {
    fn insert_identity(&mut self, identity: &mut Identity) {
        self.stage(Staged::NewIdentity(identity.snapshot()));
        identity.set_version(Version::NEW.next());
    }

    fn update_identity(&mut self, identity: &mut Identity) {
        self.stage(Staged::Identity(identity.snapshot()));
        identity.set_version(identity.version().next());
    }
}

impl StageSessions for PgBatch {
    fn insert_session(&mut self, session: &mut Session) {
        self.stage(Staged::NewSession(session.snapshot()));
        session.set_version(Version::NEW.next());
    }

    fn update_session(&mut self, session: &mut Session) {
        self.stage(Staged::Session(session.snapshot()));
        session.set_version(session.version().next());
    }

    fn revoke_sessions(&mut self, identity: IdentityId, keep: Option<SessionId>, at: Timestamp) {
        self.stage(Staged::SessionRevocation(identity, keep, at));
    }
}

impl StagePasswordResets for PgBatch {
    fn insert_password_reset(&mut self, reset: &mut PasswordReset) {
        self.stage(Staged::NewPasswordReset(reset.snapshot()));
        reset.set_version(Version::NEW.next());
    }

    fn update_password_reset(&mut self, reset: &mut PasswordReset) {
        self.stage(Staged::PasswordReset(reset.snapshot()));
        reset.set_version(reset.version().next());
    }

    fn use_up_password_resets(&mut self, identity: IdentityId, at: Timestamp) {
        self.stage(Staged::UsedUpResets(identity, at));
    }
}

impl StageCarts for PgBatch {
    fn insert_cart(&mut self, cart: &mut Cart) {
        stamp_insert(cart, self.now);
        self.stage(Staged::NewCart(cart.snapshot()));
        cart.set_version(Version::NEW.next());
    }

    fn update_cart(&mut self, cart: &mut Cart) {
        stamp_update(cart, self.now);
        self.stage(Staged::Cart(cart.snapshot()));
        cart.set_version(cart.version().next());
    }
}

impl StageProducts for PgBatch {
    fn update_product(&mut self, product: &mut Product) {
        stamp_update(product, self.now);
        self.stage(Staged::Product(product.snapshot()));
        let version = product.version().next();
        self.stage_events_of(product, version);
        product.set_version(version);
    }
}

impl StageAssetUsages for PgBatch {
    fn replace_asset_usages(
        &mut self,
        owner: &UsageOwner,
        usages: &[(hypha_core::AssetId, UsageRole)],
    ) {
        self.stage(Staged::AssetUsages(owner.clone(), usages.to_vec()));
    }
}

impl StageInventoryItems for PgBatch {
    fn update_inventory_item(&mut self, item: &mut InventoryItem) {
        stamp_update(item, self.now);
        self.stage(Staged::InventoryItem(item.snapshot()));
        let version = item.version().next();
        self.stage_events_of(item, version);
        item.set_version(version);
    }

    fn delete_inventory_item(&mut self, item: &mut InventoryItem) {
        self.stage(Staged::DeletedInventoryItem(item.snapshot()));
        let events = item.take_events();
        if !events.is_empty() {
            self.stage(Staged::Events(events, None));
        }
    }
}

impl StageOrders for PgBatch {
    fn insert_order(&mut self, order: &mut Order) {
        stamp_insert(order, self.now);
        self.stage(Staged::NewOrder(order.snapshot()));
        self.stage_events_of(order, Version::NEW.next());
        order.set_version(Version::NEW.next());
    }

    fn update_order(&mut self, order: &mut Order) {
        stamp_update(order, self.now);
        self.stage(Staged::Order(order.snapshot()));
        let version = order.version().next();
        self.stage_events_of(order, version);
        order.set_version(version);
    }
}

impl StageFulfillments for PgBatch {
    fn insert_fulfillment(&mut self, fulfillment: &mut Fulfillment) {
        stamp_insert(fulfillment, self.now);
        self.stage(Staged::NewFulfillment(fulfillment.snapshot()));
        self.stage_events_of(fulfillment, Version::NEW.next());
        fulfillment.set_version(Version::NEW.next());
    }

    fn update_fulfillment(&mut self, fulfillment: &mut Fulfillment) {
        stamp_update(fulfillment, self.now);
        self.stage(Staged::Fulfillment(fulfillment.snapshot()));
        let version = fulfillment.version().next();
        self.stage_events_of(fulfillment, version);
        fulfillment.set_version(version);
    }
}

impl StageCustomers for PgBatch {
    fn insert_customer(&mut self, customer: &mut Customer) {
        stamp_insert(customer, self.now);
        self.stage(Staged::NewCustomer(customer.snapshot()));
        self.stage_events_of(customer, Version::NEW.next());
        customer.set_version(Version::NEW.next());
    }
}

impl StageIdempotency for PgBatch {
    fn record_idempotency(&mut self, record: IdempotencyRecord) {
        self.stage(Staged::Idempotency(record));
    }
}

impl<C: PgConnection> HasBatch for PgStore<C> {
    type Batch = PgBatch;

    fn batch(&self) -> PgBatch {
        PgBatch {
            staged: Vec::new(),
            now: self.now(),
        }
    }

    /// One transaction, the writes in `Staged::cmp` order; idempotency records and
    /// inventory items (which sort last) are gathered and each written in one statement.
    /// Any failure returns early and drops the transaction, which rolls it back.
    async fn commit(&self, mut batch: PgBatch) -> Result<(), RepoError> {
        if batch.staged.is_empty() {
            return Ok(());
        }
        batch.staged.sort_by(Staged::cmp);
        let mut items = Vec::new();
        let mut deleted = Vec::new();
        let mut records = Vec::new();

        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        for write in batch.staged {
            match write {
                Staged::NewOrder(s) => insert_order(&tx, &s).await?,
                Staged::Order(s) => {
                    update_order(&tx, &s).await?;
                }
                Staged::NewFulfillment(s) => insert_fulfillment(&tx, &s).await?,
                Staged::Fulfillment(s) => {
                    update_fulfillment(&tx, &s).await?;
                }
                Staged::NewCart(s) => insert_cart(&tx, &s).await?,
                Staged::Cart(s) => {
                    update_cart(&tx, &s).await?;
                }
                Staged::Product(s) => {
                    update_product(&tx, &s).await?;
                }
                Staged::AssetUsages(owner, usages) => replace_usages(&tx, &owner, &usages).await?,
                Staged::NewCustomer(s) => insert_customer(&tx, &s).await?,
                Staged::Idempotency(record) => records.push(record),
                Staged::NewRole(s) => insert_role(&tx, &s).await?,
                Staged::Role(s) => {
                    update_role(&tx, &s).await?;
                }
                Staged::NewUser(s) => insert_user(&tx, &s).await?,
                Staged::User(s) => {
                    update_user(&tx, &s).await?;
                }
                Staged::Invite(s) => {
                    update_invite(&tx, &s).await?;
                }
                Staged::NewIdentity(s) => insert_identity(&tx, &s).await?,
                Staged::Identity(s) => {
                    update_identity(&tx, &s).await?;
                }
                Staged::NewSession(s) => insert_session(&tx, &s).await?,
                Staged::Session(s) => {
                    update_session(&tx, &s).await?;
                }
                Staged::SessionRevocation(identity, keep, at) => {
                    revoke_sessions(&tx, identity, keep, at).await?
                }
                Staged::NewPasswordReset(s) => insert_password_reset(&tx, &s).await?,
                Staged::PasswordReset(s) => update_password_reset(&tx, &s).await?,
                Staged::UsedUpResets(identity, at) => {
                    use_up_password_resets(&tx, identity, at).await?
                }
                Staged::InventoryItem(s) => items.push(s),
                Staged::DeletedInventoryItem(s) => deleted.push(s),
                Staged::TaxAssignment(products, class) => {
                    assign_tax_class(&tx, &products, class).await?
                }
                Staged::CatalogProducts(catalog, products, true) => {
                    add_catalog_products(&tx, catalog, &products).await?
                }
                Staged::CatalogProducts(catalog, products, false) => {
                    remove_catalog_products(&tx, catalog, &products).await?
                }
                Staged::Publications(channel, products, true) => {
                    crate::channels::publish(&tx, channel, &products).await?
                }
                Staged::Publications(channel, products, false) => {
                    crate::channels::unpublish(&tx, channel, &products).await?
                }
                Staged::NewCategory(s) => insert_category(&tx, &s).await?,
                Staged::Category(s) => {
                    update_category(&tx, &s).await?;
                }
                Staged::DeletedCategory(id) => delete_category(&tx, id).await?,
                Staged::CategoryProducts(category, products, true) => {
                    add_category_products(&tx, category, &products).await?
                }
                Staged::CategoryProducts(category, products, false) => {
                    remove_category_products(&tx, category, &products).await?
                }
                Staged::FixedPrices(list, changes) => {
                    let set: Vec<(VariantId, Price)> = changes
                        .iter()
                        .filter_map(|(v, p)| p.map(|p| (*v, p)))
                        .collect();
                    let removed: Vec<VariantId> = changes
                        .iter()
                        .filter(|(_, p)| p.is_none())
                        .map(|(v, _)| *v)
                        .collect();
                    if !set.is_empty() {
                        set_fixed_prices(&tx, list, &set).await?;
                    }
                    if !removed.is_empty() {
                        remove_fixed_prices(&tx, list, &removed).await?;
                    }
                }
                Staged::Events(events, version) => {
                    insert_events(&tx, &events, version, self.actor()).await?
                }
            }
        }
        insert_records(&tx, &records).await?;
        // Updated and deleted items locked together, in ID order (see `lock_items`).
        let ids: Vec<uuid::Uuid> = items
            .iter()
            .chain(&deleted)
            .map(|s| s.id.as_uuid())
            .collect();
        lock_items(&tx, &ids).await?;
        update_items(&tx, items).await?;
        delete_items(&tx, &deleted).await?;
        tx.commit().await.map_err(repo_error)
    }
}

impl StageCategories for PgBatch {
    fn insert_category(&mut self, category: &mut Category) {
        stamp_insert(category, self.now);
        let events = category.take_events();
        self.stage(Staged::NewCategory(category.snapshot()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        category.set_version(Version::NEW.next());
    }

    fn update_category(&mut self, category: &mut Category) {
        stamp_update(category, self.now);
        let version = category.version().next();
        self.stage_events_of(category, version);
        self.stage(Staged::Category(category.snapshot()));
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

impl hypha_channels::StagePublications for PgBatch {
    fn publish_products(&mut self, channel: ChannelId, products: &[ProductId]) {
        self.stage(Staged::Publications(channel, products.to_vec(), true));
    }

    fn unpublish_products(&mut self, channel: ChannelId, products: &[ProductId]) {
        self.stage(Staged::Publications(channel, products.to_vec(), false));
    }
}

impl StageCatalogProducts for PgBatch {
    fn add_catalog_products(&mut self, catalog: CatalogId, products: &[ProductId]) {
        self.stage(Staged::CatalogProducts(catalog, products.to_vec(), true));
    }

    fn remove_catalog_products(&mut self, catalog: CatalogId, products: &[ProductId]) {
        self.stage(Staged::CatalogProducts(catalog, products.to_vec(), false));
    }
}

impl StageFixedPrices for PgBatch {
    fn set_fixed_prices(&mut self, list: PriceListId, prices: &[(VariantId, Price)]) {
        let changes = prices.iter().map(|(v, p)| (*v, Some(*p))).collect();
        self.stage(Staged::FixedPrices(list, changes));
    }

    fn remove_fixed_prices(&mut self, list: PriceListId, variants: &[VariantId]) {
        let changes = variants.iter().map(|v| (*v, None)).collect();
        self.stage(Staged::FixedPrices(list, changes));
    }
}

impl StageTaxAssignments for PgBatch {
    fn assign_tax_class(&mut self, products: &[ProductId], class: Option<TaxClassId>) {
        self.stage(Staged::TaxAssignment(products.to_vec(), class));
    }
}
