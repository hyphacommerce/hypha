use std::cmp::Ordering;

use hypha_agents::{Agent, AgentSnapshot, StageAgents};
use hypha_apps::{App, AppSnapshot, StageApps};
use hypha_auth::{
    Identity, IdentitySnapshot, PasswordReset, PasswordResetSnapshot, Session, SessionSnapshot,
    StageIdentities, StagePasswordResets, StageSessions,
};
use hypha_cart::{Cart, CartSnapshot, StageCarts};
use hypha_categories::{Category, CategorySnapshot, StageCategories};
use hypha_core::{
    CartId, CatalogId, CategoryId, ChannelId, CustomFieldDefinitionId, CustomerGroupId, CustomerId,
    DiscountId, HasBatch, IdempotencyRecord, IdentityId, PendingEvent, PriceListId, ProductId,
    Recorded, RepoError, SessionId, StageEvents, StageIdempotency, TaxClassId, Timestamp,
    VariantId, Version, Versioned, stamp_insert, stamp_update,
};
use hypha_custom_fields::{
    CustomFieldDefinition, DefinitionSnapshot, StageCustomFieldDefinitions, StageCustomFieldValues,
    ValueRow,
};
use hypha_customers::{
    Customer, CustomerGroup, CustomerGroupSnapshot, CustomerSnapshot, StageCustomerGroups,
    StageCustomers,
};
use hypha_discounts::{
    ClassSet, CodeSelection, Discount, DiscountCode, DiscountSnapshot, StageDiscounts, UsageClaim,
};
use hypha_fulfillment::{Fulfillment, FulfillmentSnapshot, StageFulfillments};
use hypha_inventory::{InventoryItem, InventoryItemSnapshot, StageInventoryItems};
use hypha_markets::{FixedPrice, StageCatalogProducts, StageFixedPrices, StagePriceListSchedule};
use hypha_media::{StageAssetUsages, UsageOwner, UsageRole};
use hypha_notifications::{
    NotificationSettings, NotificationSettingsSnapshot, StageNotificationSettings,
};
use hypha_oauth::{
    AuthorizationCode, AuthorizationRequest, CodeSnapshot, OAuthToken, RequestSnapshot, StageOAuth,
    TokenSnapshot,
};
use hypha_orders::{Order, OrderSnapshot, StageOrders};
use hypha_payments::{Payment, PaymentSnapshot, StagePayments};
use hypha_products::{Product, ProductSnapshot, StageProducts};
use hypha_tax::StageTaxAssignments;
use hypha_users::{
    Invite, InviteSnapshot, Role, RoleSnapshot, StageInvites, StageRoles, StageUsers, User,
    UserSnapshot,
};

use crate::{
    auth::{
        delete_identity, insert_identity, insert_password_reset, insert_session, revoke_sessions,
        update_identity, update_password_reset, update_session, use_up_password_resets,
    },
    carts::{delete_cart, insert_cart, update_cart},
    categories::{
        add_category_products, delete_category, insert_category, remove_category_products,
        update_category,
    },
    customer_groups::{
        add_group_members, delete_group, insert_group, remove_group_members, update_group,
    },
    customers::{delete_customer, insert_customer, update_customer},
    discounts::{add_codes, claim_usage, insert_discount, remove_codes, set_codes_active},
    fulfillments::{insert_fulfillment, update_fulfillment},
    idempotency::insert_records,
    inventory::update_items,
    inventory::{delete_items, lock_items},
    markets::{
        add_catalog_products, remove_catalog_products, remove_fixed_prices, set_fixed_prices,
    },
    media::replace_usages,
    notifications::{insert_settings, update_settings},
    orders::{insert_order, update_order},
    outbox::insert_events,
    payments::{insert_payment, update_payment},
    products::{delete_product, update_product},
    store::{Db, Tx, end_transaction},
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
    NewPayment(PaymentSnapshot),
    Payment(PaymentSnapshot),
    NewCart(CartSnapshot),
    Cart(CartSnapshot),
    /// A cart removed at the version it was loaded at (its lines cascade).
    DeletedCart(CartId, Version),
    Product(ProductSnapshot),
    /// A product removed at the version it was loaded at.
    DeletedProduct(ProductId, Version),
    AssetUsages(UsageOwner, Vec<(hypha_core::AssetId, UsageRole)>),
    NewCustomer(CustomerSnapshot),
    Customer(CustomerSnapshot),
    /// A customer removed at the version it was loaded at (its addresses cascade).
    DeletedCustomer(CustomerId, Version),
    NewCustomerGroup(CustomerGroupSnapshot),
    CustomerGroup(CustomerGroupSnapshot),
    DeletedCustomerGroup(CustomerGroupId),
    CustomerGroupMembers(CustomerGroupId, Vec<CustomerId>, bool),
    Idempotency(IdempotencyRecord),
    NewRole(RoleSnapshot),
    Role(RoleSnapshot),
    NewUser(UserSnapshot),
    User(UserSnapshot),
    Invite(InviteSnapshot),
    NewIdentity(IdentitySnapshot),
    Identity(IdentitySnapshot),
    DeletedIdentity(IdentityId, Version),
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
    FixedPrices(PriceListId, Vec<(VariantId, Option<FixedPrice>)>),
    /// The schedule job's progress moved from the first to the second.
    PriceListSchedule(Option<Timestamp>, Timestamp),
    NewNotificationSettings(NotificationSettingsSnapshot),
    NotificationSettings(NotificationSettingsSnapshot),
    NewApp(AppSnapshot),
    App(AppSnapshot),
    NewOAuthRequest(RequestSnapshot),
    OAuthRequest(RequestSnapshot),
    NewOAuthCode(CodeSnapshot),
    OAuthCode(CodeSnapshot),
    NewOAuthToken(TokenSnapshot),
    OAuthToken(TokenSnapshot),
    /// Every live token of the app revoked at the time, after the single writes.
    RevokeAppTokens(String, Timestamp),
    /// Events about the aggregate staged just before, with the version that write gives
    /// it, or staged on their own (`StageEvents`, no version).
    Events(Vec<PendingEvent>, Option<Version>),
    NewCustomFieldDefinition(DefinitionSnapshot),
    CustomFieldDefinition(DefinitionSnapshot),
    /// A definition removed at the version it was loaded at (its values cascade).
    DeletedCustomFieldDefinition(CustomFieldDefinitionId, Version),
    /// A value written over what's stored (`None`), only as new (`Version::NEW`) or only
    /// over that version.
    CustomFieldValue(ValueRow, Option<Version>),
    /// A value's derived columns rewritten over that version.
    ReindexedCustomFieldValue(ValueRow, Version),
    RemovedCustomFieldValue(CustomFieldDefinitionId, uuid::Uuid, Option<Version>),
    NewAgent(AgentSnapshot),
    /// A new discount with its classes (derived, stored for lists).
    NewDiscount(DiscountSnapshot, ClassSet),
    DiscountCodes(Vec<DiscountCode>),
    RemovedDiscountCodes(DiscountId, CodeSelection),
    DiscountUsage(UsageClaim),
    DiscountCodesActive(DiscountId, CodeSelection, bool),
    /// The discount schedule job's progress moved from the first to the second.
    DiscountSchedule(Option<Timestamp>, Timestamp),
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
            Staged::NewPayment(_) => 4,
            Staged::Payment(_) => 5,
            Staged::NewCart(_) => 6,
            Staged::Cart(_) | Staged::DeletedCart(..) => 7,
            Staged::Product(_) | Staged::DeletedProduct(..) => 8,
            // The one settings row, before the usages staged with it (its logo's).
            Staged::NewNotificationSettings(_) | Staged::NotificationSettings(_) => 9,
            Staged::AssetUsages(..) => 10,
            Staged::NewCustomer(_) | Staged::Customer(_) | Staged::DeletedCustomer(..) => 11,
            // Groups after their customers (membership references both), before their
            // membership.
            Staged::NewCustomerGroup(_)
            | Staged::CustomerGroup(_)
            | Staged::DeletedCustomerGroup(_) => 25,
            Staged::CustomerGroupMembers(..) => 28,
            Staged::NewRole(_) => 12,
            Staged::Role(_) => 13,
            Staged::NewUser(_) => 14,
            Staged::User(_) => 15,
            Staged::Invite(_) => 16,
            Staged::NewIdentity(_) => 17,
            Staged::Identity(_) | Staged::DeletedIdentity(..) => 18,
            Staged::NewSession(_) => 19,
            Staged::Session(_) => 20,
            Staged::SessionRevocation(..) => 21,
            Staged::NewPasswordReset(_) => 22,
            Staged::PasswordReset(_) => 23,
            Staged::UsedUpResets(..) => 24,
            // Categories before their membership (a new one must exist first).
            Staged::NewCategory(_) => 25,
            Staged::Category(_) => 26,
            Staged::DeletedCategory(_) => 27,
            Staged::CategoryProducts(..) => 28,
            // Set-based writes (memberships, fixed prices, tax classes): one statement
            // each, whatever the number of rows.
            Staged::CatalogProducts(..) => 29,
            Staged::Publications(..) => 30,
            Staged::FixedPrices(..) => 31,
            Staged::PriceListSchedule(..) => 31,
            Staged::TaxAssignment(..) => 32,
            Staged::Idempotency(_) => 33,
            Staged::InventoryItem(_) => 34,
            Staged::DeletedInventoryItem(_) => 35,
            // Inserts into a table nothing else locks: anywhere would do.
            Staged::Events(..) => 36,
            // An app before what refers to it (its tokens), a request before its codes.
            Staged::NewApp(_) => 37,
            Staged::App(_) => 38,
            Staged::NewOAuthRequest(_) => 39,
            Staged::OAuthRequest(_) => 40,
            Staged::NewOAuthCode(_) => 41,
            Staged::OAuthCode(_) => 42,
            Staged::NewOAuthToken(_) => 43,
            Staged::OAuthToken(_) => 44,
            Staged::RevokeAppTokens(..) => 45,
            // Definitions after the apps that manage them, values after definitions.
            Staged::NewCustomFieldDefinition(_) => 46,
            Staged::CustomFieldDefinition(_) => 47,
            Staged::DeletedCustomFieldDefinition(..) => 48,
            Staged::CustomFieldValue(..)
            | Staged::ReindexedCustomFieldValue(..)
            | Staged::RemovedCustomFieldValue(..) => 49,
            // An agent before its proposals.
            Staged::NewAgent(_) => 50,
            // A discount before its codes; usage claims last, by discount, so two checkouts
            // lock counters in the same order.
            Staged::NewDiscount(..) => 51,
            Staged::DiscountCodes(_)
            | Staged::RemovedDiscountCodes(..)
            | Staged::DiscountCodesActive(..) => 52,
            Staged::DiscountUsage(_) => 53,
            Staged::DiscountSchedule(..) => 54,
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
                (Staged::NewPayment(a), Staged::NewPayment(b))
                | (Staged::Payment(a), Staged::Payment(b)) => a.id.cmp(&b.id),
                (Staged::NewCart(a), Staged::NewCart(b)) | (Staged::Cart(a), Staged::Cart(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::DeletedCart(a, _), Staged::DeletedCart(b, _)) => a.cmp(b),
                (Staged::Product(a), Staged::Product(b)) => a.id.cmp(&b.id),
                (Staged::DeletedProduct(a, _), Staged::DeletedProduct(b, _)) => a.cmp(b),
                (Staged::AssetUsages(a, _), Staged::AssetUsages(b, _)) => a.cmp(b),
                (Staged::NewCustomer(a), Staged::NewCustomer(b))
                | (Staged::Customer(a), Staged::Customer(b)) => a.id.cmp(&b.id),
                (Staged::DeletedCustomer(a, _), Staged::DeletedCustomer(b, _)) => a.cmp(b),
                (Staged::NewCustomerGroup(a), Staged::NewCustomerGroup(b))
                | (Staged::CustomerGroup(a), Staged::CustomerGroup(b)) => a.id.cmp(&b.id),
                (Staged::DeletedCustomerGroup(a), Staged::DeletedCustomerGroup(b)) => a.cmp(b),
                (Staged::CustomerGroupMembers(a, ..), Staged::CustomerGroupMembers(b, ..)) => {
                    a.cmp(b)
                }
                (Staged::NewRole(a), Staged::NewRole(b)) | (Staged::Role(a), Staged::Role(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::NewUser(a), Staged::NewUser(b)) | (Staged::User(a), Staged::User(b)) => {
                    a.id.cmp(&b.id)
                }
                (Staged::Invite(a), Staged::Invite(b)) => a.id.cmp(&b.id),
                (Staged::NewIdentity(a), Staged::NewIdentity(b))
                | (Staged::Identity(a), Staged::Identity(b)) => a.id.cmp(&b.id),
                (Staged::DeletedIdentity(a, _), Staged::DeletedIdentity(b, _)) => a.cmp(b),
                (Staged::Identity(a), Staged::DeletedIdentity(b, _)) => a.id.cmp(b),
                (Staged::DeletedIdentity(a, _), Staged::Identity(b)) => a.cmp(&b.id),
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
                (Staged::NewApp(a), Staged::NewApp(b)) | (Staged::App(a), Staged::App(b)) => {
                    a.key.cmp(&b.key)
                }
                (Staged::NewOAuthRequest(a), Staged::NewOAuthRequest(b))
                | (Staged::OAuthRequest(a), Staged::OAuthRequest(b)) => a.id.cmp(&b.id),
                (Staged::NewOAuthToken(a), Staged::NewOAuthToken(b))
                | (Staged::OAuthToken(a), Staged::OAuthToken(b)) => a.id.cmp(&b.id),
                (Staged::NewCustomFieldDefinition(a), Staged::NewCustomFieldDefinition(b))
                | (Staged::CustomFieldDefinition(a), Staged::CustomFieldDefinition(b)) => {
                    a.id.cmp(&b.id)
                }
                (
                    Staged::DeletedCustomFieldDefinition(a, _),
                    Staged::DeletedCustomFieldDefinition(b, _),
                ) => a.cmp(b),
                (
                    Staged::CustomFieldValue(..)
                    | Staged::ReindexedCustomFieldValue(..)
                    | Staged::RemovedCustomFieldValue(..),
                    Staged::CustomFieldValue(..)
                    | Staged::ReindexedCustomFieldValue(..)
                    | Staged::RemovedCustomFieldValue(..),
                ) => value_key(self).cmp(&value_key(other)),
                (Staged::NewAgent(a), Staged::NewAgent(b)) => a.id.cmp(&b.id),
                (Staged::NewDiscount(a, _), Staged::NewDiscount(b, _)) => a.id.cmp(&b.id),
                (Staged::DiscountUsage(a), Staged::DiscountUsage(b)) => a
                    .discount
                    .cmp(&b.discount)
                    .then_with(|| a.code.cmp(&b.code)),
                _ => Ordering::Equal,
            })
    }
}

/// A value write's row: (definition, owner).
fn value_key(write: &Staged) -> Option<(CustomFieldDefinitionId, uuid::Uuid)> {
    match write {
        Staged::CustomFieldValue(row, _) | Staged::ReindexedCustomFieldValue(row, _) => {
            Some((row.definition, row.owner))
        }
        Staged::RemovedCustomFieldValue(definition, owner, _) => Some((*definition, *owner)),
        _ => None,
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

    fn delete_identity(&mut self, identity: &Identity) {
        self.stage(Staged::DeletedIdentity(identity.id(), identity.version()));
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

    fn delete_cart(&mut self, cart: &Cart) {
        self.stage(Staged::DeletedCart(cart.id(), cart.version()));
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

    fn delete_product(&mut self, product: &mut Product) {
        self.stage(Staged::DeletedProduct(product.id(), product.version()));
        let events = product.take_events();
        if !events.is_empty() {
            self.stage(Staged::Events(events, None));
        }
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

impl StagePayments for PgBatch {
    fn insert_payment(&mut self, payment: &mut Payment) {
        stamp_insert(payment, self.now);
        self.stage(Staged::NewPayment(payment.snapshot()));
        self.stage_events_of(payment, Version::NEW.next());
        payment.set_version(Version::NEW.next());
    }

    fn update_payment(&mut self, payment: &mut Payment) {
        stamp_update(payment, self.now);
        self.stage(Staged::Payment(payment.snapshot()));
        let version = payment.version().next();
        self.stage_events_of(payment, version);
        payment.set_version(version);
    }
}

impl StageCustomers for PgBatch {
    fn insert_customer(&mut self, customer: &mut Customer) {
        stamp_insert(customer, self.now);
        self.stage(Staged::NewCustomer(customer.snapshot()));
        self.stage_events_of(customer, Version::NEW.next());
        customer.set_version(Version::NEW.next());
    }

    fn update_customer(&mut self, customer: &mut Customer) {
        stamp_update(customer, self.now);
        let version = customer.version().next();
        self.stage_events_of(customer, version);
        self.stage(Staged::Customer(customer.snapshot()));
        customer.set_version(version);
    }

    fn delete_customer(&mut self, customer: &mut Customer) {
        self.stage(Staged::DeletedCustomer(customer.id(), customer.version()));
        self.stage_events_of(customer, customer.version());
    }
}

impl StageCustomerGroups for PgBatch {
    fn insert_group(&mut self, group: &mut CustomerGroup) {
        stamp_insert(group, self.now);
        let events = group.take_events();
        self.stage(Staged::NewCustomerGroup(group.snapshot()));
        self.stage(Staged::Events(events, Some(Version::NEW.next())));
        group.set_version(Version::NEW.next());
    }

    fn update_group(&mut self, group: &mut CustomerGroup) {
        stamp_update(group, self.now);
        let version = group.version().next();
        self.stage_events_of(group, version);
        self.stage(Staged::CustomerGroup(group.snapshot()));
        group.set_version(version);
    }

    fn delete_group(&mut self, group: CustomerGroupId) {
        self.stage(Staged::DeletedCustomerGroup(group));
    }

    fn add_group_members(&mut self, group: CustomerGroupId, customers: &[CustomerId]) {
        self.stage(Staged::CustomerGroupMembers(
            group,
            customers.to_vec(),
            true,
        ));
    }

    fn remove_group_members(&mut self, group: CustomerGroupId, customers: &[CustomerId]) {
        self.stage(Staged::CustomerGroupMembers(
            group,
            customers.to_vec(),
            false,
        ));
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
    /// Any failure stops the writes and rolls the transaction back (`end_transaction`,
    /// awaited: never by dropping it).
    async fn commit(&self, mut batch: PgBatch) -> Result<(), RepoError> {
        if batch.staged.is_empty() {
            return Ok(());
        }
        batch.staged.sort_by(Staged::cmp);
        let now = batch.now;
        let mut items = Vec::new();
        let mut deleted = Vec::new();
        let mut records = Vec::new();

        let mut conn = self.conn().await;
        let preview = conn.in_preview();
        let (client, statements) = conn.parts();
        let tx = Tx::begin(client, preview).await?;
        let db = Db::new(tx.client(), statements);
        // Every early return ends the transaction explicitly (`end_transaction`).
        let written: Result<(), RepoError> = async {
            for write in batch.staged {
                match write {
                    Staged::NewOrder(s) => insert_order(&db, &s).await?,
                    Staged::Order(s) => {
                        update_order(&db, &s).await?;
                    }
                    Staged::NewFulfillment(s) => insert_fulfillment(&db, &s).await?,
                    Staged::Fulfillment(s) => {
                        update_fulfillment(&db, &s).await?;
                    }
                    Staged::NewPayment(s) => insert_payment(&db, &s).await?,
                    Staged::Payment(s) => {
                        update_payment(&db, &s).await?;
                    }
                    Staged::NewCart(s) => insert_cart(&db, &s).await?,
                    Staged::Cart(s) => {
                        update_cart(&db, &s).await?;
                    }
                    Staged::DeletedCart(id, version) => delete_cart(&db, id, version).await?,
                    Staged::Product(s) => {
                        update_product(&db, &s).await?;
                    }
                    Staged::DeletedProduct(id, version) => delete_product(&db, id, version).await?,
                    Staged::NewNotificationSettings(s) => insert_settings(&db, &s).await?,
                    Staged::NotificationSettings(s) => {
                        update_settings(&db, &s).await?;
                    }
                    Staged::AssetUsages(owner, usages) => {
                        replace_usages(&db, &owner, &usages).await?
                    }
                    Staged::NewCustomer(s) => insert_customer(&db, &s).await?,
                    Staged::Customer(s) => {
                        update_customer(&db, &s).await?;
                    }
                    Staged::DeletedCustomer(id, version) => {
                        delete_customer(&db, id, version).await?
                    }
                    Staged::Idempotency(record) => records.push(record),
                    Staged::NewRole(s) => insert_role(&db, &s).await?,
                    Staged::Role(s) => {
                        update_role(&db, &s).await?;
                    }
                    Staged::NewUser(s) => insert_user(&db, &s).await?,
                    Staged::User(s) => {
                        update_user(&db, &s).await?;
                    }
                    Staged::Invite(s) => {
                        update_invite(&db, &s).await?;
                    }
                    Staged::NewIdentity(s) => insert_identity(&db, &s).await?,
                    Staged::Identity(s) => {
                        update_identity(&db, &s).await?;
                    }
                    Staged::DeletedIdentity(id, version) => {
                        delete_identity(&db, id, version).await?
                    }
                    Staged::NewSession(s) => insert_session(&db, &s).await?,
                    Staged::Session(s) => {
                        update_session(&db, &s).await?;
                    }
                    Staged::SessionRevocation(identity, keep, at) => {
                        revoke_sessions(&db, identity, keep, at).await?
                    }
                    Staged::NewPasswordReset(s) => insert_password_reset(&db, &s).await?,
                    Staged::PasswordReset(s) => update_password_reset(&db, &s).await?,
                    Staged::UsedUpResets(identity, at) => {
                        use_up_password_resets(&db, identity, at).await?
                    }
                    Staged::InventoryItem(s) => items.push(s),
                    Staged::DeletedInventoryItem(s) => deleted.push(s),
                    Staged::TaxAssignment(products, class) => {
                        assign_tax_class(&db, &products, class).await?
                    }
                    Staged::CatalogProducts(catalog, products, true) => {
                        add_catalog_products(&db, catalog, &products).await?
                    }
                    Staged::CatalogProducts(catalog, products, false) => {
                        remove_catalog_products(&db, catalog, &products).await?
                    }
                    Staged::Publications(channel, products, true) => {
                        crate::channels::publish(&db, channel, &products).await?
                    }
                    Staged::Publications(channel, products, false) => {
                        crate::channels::unpublish(&db, channel, &products).await?
                    }
                    Staged::NewCategory(s) => insert_category(&db, &s).await?,
                    Staged::Category(s) => {
                        update_category(&db, &s).await?;
                    }
                    Staged::DeletedCategory(id) => delete_category(&db, id).await?,
                    Staged::CategoryProducts(category, products, true) => {
                        add_category_products(&db, category, &products).await?
                    }
                    Staged::CategoryProducts(category, products, false) => {
                        remove_category_products(&db, category, &products).await?
                    }
                    Staged::NewCustomerGroup(s) => insert_group(&db, &s).await?,
                    Staged::CustomerGroup(s) => {
                        update_group(&db, &s).await?;
                    }
                    Staged::DeletedCustomerGroup(id) => delete_group(&db, id).await?,
                    Staged::CustomerGroupMembers(group, customers, true) => {
                        add_group_members(&db, group, &customers).await?
                    }
                    Staged::CustomerGroupMembers(group, customers, false) => {
                        remove_group_members(&db, group, &customers).await?
                    }
                    Staged::PriceListSchedule(from, to) => {
                        crate::markets::advance_price_list_schedule(&db, from, to).await?
                    }
                    Staged::FixedPrices(list, changes) => {
                        let set: Vec<(VariantId, FixedPrice)> = changes
                            .iter()
                            .filter_map(|(v, p)| p.clone().map(|p| (*v, p)))
                            .collect();
                        let removed: Vec<VariantId> = changes
                            .iter()
                            .filter(|(_, p)| p.is_none())
                            .map(|(v, _)| *v)
                            .collect();
                        if !set.is_empty() {
                            set_fixed_prices(&db, list, &set).await?;
                        }
                        if !removed.is_empty() {
                            remove_fixed_prices(&db, list, &removed).await?;
                        }
                    }
                    Staged::Events(events, version) => {
                        insert_events(&db, &events, version, self.event_stamp()).await?
                    }
                    Staged::NewApp(s) => crate::apps::insert_app(&db, &s).await?,
                    Staged::App(s) => {
                        crate::apps::update_app(&db, &s).await?;
                    }
                    Staged::NewOAuthRequest(s) => crate::oauth::insert_request(&db, &s).await?,
                    Staged::OAuthRequest(s) => crate::oauth::update_request(&db, &s).await?,
                    Staged::NewOAuthCode(s) => crate::oauth::insert_code(&db, &s).await?,
                    Staged::OAuthCode(s) => crate::oauth::update_code(&db, &s).await?,
                    Staged::NewOAuthToken(s) => crate::oauth::insert_token(&db, &s).await?,
                    Staged::OAuthToken(s) => crate::oauth::update_token(&db, &s).await?,
                    Staged::RevokeAppTokens(app, at) => {
                        crate::oauth::revoke_app_tokens(&db, &app, at).await?;
                    }
                    Staged::NewCustomFieldDefinition(s) => {
                        crate::custom_fields::insert_definition(&db, &s).await?
                    }
                    Staged::CustomFieldDefinition(s) => {
                        crate::custom_fields::update_definition(&db, &s).await?;
                    }
                    Staged::DeletedCustomFieldDefinition(id, version) => {
                        crate::custom_fields::delete_definition(&db, id, version).await?
                    }
                    Staged::CustomFieldValue(row, expected) => {
                        crate::custom_fields::set_value(&db, &row, expected, now).await?
                    }
                    Staged::ReindexedCustomFieldValue(row, at) => {
                        crate::custom_fields::reindex_value(&db, &row, at).await?
                    }
                    Staged::RemovedCustomFieldValue(definition, owner, expected) => {
                        crate::custom_fields::remove_value(&db, definition, owner, expected).await?
                    }
                    Staged::NewAgent(s) => crate::agents::insert_agent(&db, &s).await?,
                    Staged::NewDiscount(s, classes) => insert_discount(&db, &s, classes).await?,
                    Staged::DiscountCodes(codes) => add_codes(&db, &codes).await?,
                    Staged::RemovedDiscountCodes(discount, selection) => {
                        remove_codes(&db, discount, &selection).await?
                    }
                    Staged::DiscountUsage(claim) => claim_usage(&db, &claim).await?,
                    Staged::DiscountCodesActive(discount, selection, active) => {
                        set_codes_active(&db, discount, &selection, active).await?
                    }
                    Staged::DiscountSchedule(from, to) => {
                        crate::discounts::advance_discount_schedule(&db, from, to).await?
                    }
                }
            }
            insert_records(&db, &records).await?;
            // Updated and deleted items locked together, in ID order (see `lock_items`).
            let ids: Vec<uuid::Uuid> = items
                .iter()
                .chain(&deleted)
                .map(|s| s.id.as_uuid())
                .collect();
            lock_items(&db, &ids).await?;
            update_items(&db, items).await?;
            delete_items(&db, &deleted).await?;
            Ok(())
        }
        .await;
        let ended = end_transaction(tx, written).await;
        self.transaction_ended(ended.is_ok());
        ended
    }
}

impl StageNotificationSettings for PgBatch {
    fn insert_notification_settings(&mut self, settings: &mut NotificationSettings) {
        stamp_insert(settings, self.now);
        self.stage(Staged::NewNotificationSettings(settings.snapshot()));
        settings.set_version(Version::NEW.next());
    }

    fn update_notification_settings(&mut self, settings: &mut NotificationSettings) {
        stamp_update(settings, self.now);
        self.stage(Staged::NotificationSettings(settings.snapshot()));
        settings.set_version(settings.version().next());
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

impl StagePriceListSchedule for PgBatch {
    fn advance_price_list_schedule(&mut self, from: Option<Timestamp>, to: Timestamp) {
        self.stage(Staged::PriceListSchedule(from, to));
    }
}

impl StageFixedPrices for PgBatch {
    fn set_fixed_prices(&mut self, list: PriceListId, prices: &[(VariantId, FixedPrice)]) {
        let changes = prices.iter().map(|(v, p)| (*v, Some(p.clone()))).collect();
        self.stage(Staged::FixedPrices(list, changes));
    }

    fn remove_fixed_prices(&mut self, list: PriceListId, variants: &[VariantId]) {
        let changes = variants.iter().map(|v| (*v, None)).collect();
        self.stage(Staged::FixedPrices(list, changes));
    }
}

impl StageApps for PgBatch {
    fn insert_app(&mut self, app: &mut App) {
        stamp_insert(app, self.now);
        self.stage(Staged::NewApp(app.snapshot()));
        self.stage_events_of(app, Version::NEW.next());
        app.set_version(Version::NEW.next());
    }

    fn update_app(&mut self, app: &mut App) {
        stamp_update(app, self.now);
        self.stage(Staged::App(app.snapshot()));
        let version = app.version().next();
        self.stage_events_of(app, version);
        app.set_version(version);
    }
}

impl StageOAuth for PgBatch {
    fn insert_oauth_request(&mut self, request: &mut AuthorizationRequest) {
        self.stage(Staged::NewOAuthRequest(request.snapshot()));
        request.set_version(Version::NEW.next());
    }

    fn update_oauth_request(&mut self, request: &mut AuthorizationRequest) {
        self.stage(Staged::OAuthRequest(request.snapshot()));
        request.set_version(request.version().next());
    }

    fn insert_oauth_code(&mut self, code: &mut AuthorizationCode) {
        self.stage(Staged::NewOAuthCode(code.snapshot()));
        code.set_version(Version::NEW.next());
    }

    fn update_oauth_code(&mut self, code: &mut AuthorizationCode) {
        self.stage(Staged::OAuthCode(code.snapshot()));
        code.set_version(code.version().next());
    }

    fn insert_oauth_token(&mut self, token: &mut OAuthToken) {
        self.stage(Staged::NewOAuthToken(token.snapshot()));
        token.set_version(Version::NEW.next());
    }

    fn update_oauth_token(&mut self, token: &mut OAuthToken) {
        self.stage(Staged::OAuthToken(token.snapshot()));
        token.set_version(token.version().next());
    }

    fn revoke_app_tokens(&mut self, app: &hypha_core::AppKey, at: Timestamp) {
        self.stage(Staged::RevokeAppTokens(app.as_str().to_string(), at));
    }
}

impl StageTaxAssignments for PgBatch {
    fn assign_tax_class(&mut self, products: &[ProductId], class: Option<TaxClassId>) {
        self.stage(Staged::TaxAssignment(products.to_vec(), class));
    }
}

impl StageCustomFieldDefinitions for PgBatch {
    fn insert_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition) {
        stamp_insert(definition, self.now);
        self.stage(Staged::NewCustomFieldDefinition(definition.snapshot()));
        self.stage_events_of(definition, Version::NEW.next());
        definition.set_version(Version::NEW.next());
    }

    fn update_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition) {
        stamp_update(definition, self.now);
        self.stage(Staged::CustomFieldDefinition(definition.snapshot()));
        let version = definition.version().next();
        self.stage_events_of(definition, version);
        definition.set_version(version);
    }

    fn delete_custom_field_definition(&mut self, definition: &mut CustomFieldDefinition) {
        self.stage(Staged::DeletedCustomFieldDefinition(
            definition.id(),
            definition.version(),
        ));
        let events = definition.take_events();
        if !events.is_empty() {
            self.stage(Staged::Events(events, None));
        }
    }
}

impl StageCustomFieldValues for PgBatch {
    fn set_custom_field_value(&mut self, row: ValueRow, expected: Option<Version>) {
        self.stage(Staged::CustomFieldValue(row, expected));
    }

    fn remove_custom_field_value(
        &mut self,
        definition: CustomFieldDefinitionId,
        owner: uuid::Uuid,
        expected: Option<Version>,
    ) {
        self.stage(Staged::RemovedCustomFieldValue(definition, owner, expected));
    }

    fn reindex_custom_field_value(&mut self, row: ValueRow, at: Version) {
        self.stage(Staged::ReindexedCustomFieldValue(row, at));
    }
}

impl StageDiscounts for PgBatch {
    fn insert_discount(&mut self, discount: &mut Discount) {
        stamp_insert(discount, self.now);
        self.stage(Staged::NewDiscount(discount.snapshot(), discount.classes()));
        self.stage_events_of(discount, Version::NEW.next());
        discount.set_version(Version::NEW.next());
    }

    fn add_discount_codes(&mut self, codes: &[DiscountCode]) {
        self.stage(Staged::DiscountCodes(codes.to_vec()));
    }

    fn remove_discount_codes(&mut self, discount: DiscountId, codes: CodeSelection) {
        self.stage(Staged::RemovedDiscountCodes(discount, codes));
    }

    fn set_discount_codes_active(
        &mut self,
        discount: DiscountId,
        codes: CodeSelection,
        active: bool,
    ) {
        self.stage(Staged::DiscountCodesActive(discount, codes, active));
    }

    fn claim_discount_usage(&mut self, claim: UsageClaim) {
        self.stage(Staged::DiscountUsage(claim));
    }

    fn advance_discount_schedule(&mut self, from: Option<Timestamp>, to: Timestamp) {
        self.stage(Staged::DiscountSchedule(from, to));
    }
}

impl StageAgents for PgBatch {
    fn insert_agent(&mut self, agent: &mut Agent) {
        stamp_insert(agent, self.now);
        self.stage(Staged::NewAgent(agent.snapshot()));
        self.stage_events_of(agent, Version::NEW.next());
        agent.set_version(Version::NEW.next());
    }
}
