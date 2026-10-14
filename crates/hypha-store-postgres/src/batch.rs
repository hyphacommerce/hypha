use hypha_auth::{
    Identity, IdentitySnapshot, PasswordReset, PasswordResetSnapshot, Session, SessionSnapshot,
    StageIdentities, StagePasswordResets, StageSessions,
};
use hypha_cart::{Cart, CartSnapshot, StageCarts};
use hypha_core::{
    HasBatch, IdempotencyRecord, IdentityId, RepoError, SessionId, StageIdempotency, Timestamp,
    Version, Versioned,
};
use hypha_customers::{Customer, CustomerSnapshot, StageCustomers};
use hypha_inventory::{InventoryItem, InventoryItemSnapshot, StageInventoryItems};
use hypha_orders::{Order, OrderSnapshot, StageOrders};
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
    customers::insert_customer,
    error::repo_error,
    idempotency::insert_records,
    inventory::update_items,
    orders::insert_order,
    store::{PgConnection, PgStore},
    users::{insert_role, insert_user, update_invite, update_role, update_user},
};

/// Writes staged for `PgStore::commit`: snapshots taken when each aggregate was staged,
/// carrying the version it was loaded at.
#[derive(Default)]
pub struct PgBatch {
    new_orders: Vec<OrderSnapshot>,
    new_carts: Vec<CartSnapshot>,
    new_customers: Vec<CustomerSnapshot>,
    carts: Vec<CartSnapshot>,
    inventory_items: Vec<InventoryItemSnapshot>,
    idempotency: Vec<IdempotencyRecord>,
    new_roles: Vec<RoleSnapshot>,
    roles: Vec<RoleSnapshot>,
    new_users: Vec<UserSnapshot>,
    users: Vec<UserSnapshot>,
    invites: Vec<InviteSnapshot>,
    new_identities: Vec<IdentitySnapshot>,
    identities: Vec<IdentitySnapshot>,
    new_sessions: Vec<SessionSnapshot>,
    sessions: Vec<SessionSnapshot>,
    session_revocations: Vec<(IdentityId, Option<SessionId>, Timestamp)>,
    new_password_resets: Vec<PasswordResetSnapshot>,
    password_resets: Vec<PasswordResetSnapshot>,
    used_up_resets: Vec<(IdentityId, Timestamp)>,
}

impl PgBatch {
    fn is_empty(&self) -> bool {
        self.new_orders.is_empty()
            && self.new_carts.is_empty()
            && self.new_customers.is_empty()
            && self.carts.is_empty()
            && self.inventory_items.is_empty()
            && self.idempotency.is_empty()
            && self.new_roles.is_empty()
            && self.roles.is_empty()
            && self.new_users.is_empty()
            && self.users.is_empty()
            && self.invites.is_empty()
            && self.new_identities.is_empty()
            && self.identities.is_empty()
            && self.new_sessions.is_empty()
            && self.sessions.is_empty()
            && self.session_revocations.is_empty()
            && self.new_password_resets.is_empty()
            && self.password_resets.is_empty()
            && self.used_up_resets.is_empty()
    }
}

impl StageRoles for PgBatch {
    fn insert_role(&mut self, role: &mut Role) {
        self.new_roles.push(role.snapshot());
        role.set_version(Version::NEW.next());
    }

    fn update_role(&mut self, role: &mut Role) {
        self.roles.push(role.snapshot());
        role.set_version(role.version().next());
    }
}

impl StageUsers for PgBatch {
    fn insert_user(&mut self, user: &mut User) {
        self.new_users.push(user.snapshot());
        user.set_version(Version::NEW.next());
    }

    fn update_user(&mut self, user: &mut User) {
        self.users.push(user.snapshot());
        user.set_version(user.version().next());
    }
}

impl StageInvites for PgBatch {
    fn update_invite(&mut self, invite: &mut Invite) {
        self.invites.push(invite.snapshot());
        invite.set_version(invite.version().next());
    }
}

impl StageIdentities for PgBatch {
    fn insert_identity(&mut self, identity: &mut Identity) {
        self.new_identities.push(identity.snapshot());
        identity.set_version(Version::NEW.next());
    }

    fn update_identity(&mut self, identity: &mut Identity) {
        self.identities.push(identity.snapshot());
        identity.set_version(identity.version().next());
    }
}

impl StageSessions for PgBatch {
    fn insert_session(&mut self, session: &mut Session) {
        self.new_sessions.push(session.snapshot());
        session.set_version(Version::NEW.next());
    }

    fn update_session(&mut self, session: &mut Session) {
        self.sessions.push(session.snapshot());
        session.set_version(session.version().next());
    }

    fn revoke_sessions(&mut self, identity: IdentityId, keep: Option<SessionId>, at: Timestamp) {
        self.session_revocations.push((identity, keep, at));
    }
}

impl StagePasswordResets for PgBatch {
    fn insert_password_reset(&mut self, reset: &mut PasswordReset) {
        self.new_password_resets.push(reset.snapshot());
        reset.set_version(Version::NEW.next());
    }

    fn update_password_reset(&mut self, reset: &mut PasswordReset) {
        self.password_resets.push(reset.snapshot());
        reset.set_version(reset.version().next());
    }

    fn use_up_password_resets(&mut self, identity: IdentityId, at: Timestamp) {
        self.used_up_resets.push((identity, at));
    }
}

impl StageCarts for PgBatch {
    fn insert_cart(&mut self, cart: &mut Cart) {
        self.new_carts.push(cart.snapshot());
        cart.set_version(Version::NEW.next());
    }

    fn update_cart(&mut self, cart: &mut Cart) {
        self.carts.push(cart.snapshot());
        cart.set_version(cart.version().next());
    }
}

impl StageInventoryItems for PgBatch {
    fn update_inventory_item(&mut self, item: &mut InventoryItem) {
        self.inventory_items.push(item.snapshot());
        item.set_version(item.version().next());
    }
}

impl StageOrders for PgBatch {
    fn insert_order(&mut self, order: &mut Order) {
        self.new_orders.push(order.snapshot());
        order.set_version(Version::NEW.next());
    }
}

impl StageCustomers for PgBatch {
    fn insert_customer(&mut self, customer: &mut Customer) {
        self.new_customers.push(customer.snapshot());
        customer.set_version(Version::NEW.next());
    }
}

impl StageIdempotency for PgBatch {
    fn record_idempotency(&mut self, record: IdempotencyRecord) {
        self.idempotency.push(record);
    }
}

impl<C: PgConnection> HasBatch for PgStore<C> {
    type Batch = PgBatch;

    fn batch(&self) -> PgBatch {
        PgBatch::default()
    }

    /// One transaction. Tables are written in a fixed order, and rows within a table by
    /// ID, so two commits lock rows in the same order and queue instead of deadlocking.
    /// Any failure returns early and drops the transaction, which rolls it back.
    async fn commit(&self, mut batch: PgBatch) -> Result<(), RepoError> {
        if batch.is_empty() {
            return Ok(());
        }
        batch.carts.sort_by_key(|s| s.id.as_uuid());

        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        for order in &batch.new_orders {
            insert_order(&tx, order).await?;
        }
        for cart in &batch.new_carts {
            insert_cart(&tx, cart).await?;
        }
        for cart in &batch.carts {
            update_cart(&tx, cart).await?;
        }
        update_items(&tx, batch.inventory_items).await?;
        for customer in &batch.new_customers {
            insert_customer(&tx, customer).await?;
        }
        insert_records(&tx, &batch.idempotency).await?;
        for role in &batch.new_roles {
            insert_role(&tx, role).await?;
        }
        for role in &batch.roles {
            update_role(&tx, role).await?;
        }
        for user in &batch.new_users {
            insert_user(&tx, user).await?;
        }
        for user in &batch.users {
            update_user(&tx, user).await?;
        }
        for invite in &batch.invites {
            update_invite(&tx, invite).await?;
        }
        for identity in &batch.new_identities {
            insert_identity(&tx, identity).await?;
        }
        for identity in &batch.identities {
            update_identity(&tx, identity).await?;
        }
        for session in &batch.new_sessions {
            insert_session(&tx, session).await?;
        }
        for session in &batch.sessions {
            update_session(&tx, session).await?;
        }
        for (identity, keep, at) in &batch.session_revocations {
            revoke_sessions(&tx, *identity, *keep, *at).await?;
        }
        for reset in &batch.new_password_resets {
            insert_password_reset(&tx, reset).await?;
        }
        for reset in &batch.password_resets {
            update_password_reset(&tx, reset).await?;
        }
        for (identity, at) in &batch.used_up_resets {
            use_up_password_resets(&tx, *identity, *at).await?;
        }
        tx.commit().await.map_err(repo_error)
    }
}
