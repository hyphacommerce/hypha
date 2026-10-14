use hypha_auth::{
    Identity, PasswordReset, Session, StageIdentities, StagePasswordResets, StageSessions,
    keys as auth_keys,
};
use hypha_cart::{Cart, StageCarts, repo::keys as cart_keys};
use hypha_core::{
    HasBatch, IdempotencyRecord, IdentityId, RepoError, SessionId, StageIdempotency, Timestamp,
    Version, Versioned,
};
use hypha_customers::{Customer, StageCustomers, repo::keys as customer_keys};
use hypha_inventory::{InventoryItem, StageInventoryItems};
use hypha_orders::{Order, StageOrders, repo::keys as order_keys};
use hypha_users::{Invite, Role, StageInvites, StageRoles, StageUsers, User, keys as user_keys};

use crate::{
    InMemoryPorts,
    auth::{unique_identity, unique_reset, unique_session},
    customers::unique as unique_customer,
    idempotency::insert_record,
    inventory_items,
    orders::unique as unique_order,
    table::{insert_row, no_unique_keys, update_row},
    users::{unique_invite, unique_role, unique_user},
};

/// Writes staged for `InMemoryPorts::commit`: copies of the aggregates as they were
/// staged, still carrying the version they were loaded at.
#[derive(Default)]
pub struct InMemoryBatch {
    new_orders: Vec<Order>,
    new_carts: Vec<Cart>,
    new_customers: Vec<Customer>,
    carts: Vec<Cart>,
    inventory_items: Vec<InventoryItem>,
    idempotency: Vec<IdempotencyRecord>,
    new_identities: Vec<Identity>,
    identities: Vec<Identity>,
    new_sessions: Vec<Session>,
    sessions: Vec<Session>,
    session_revocations: Vec<(IdentityId, Option<SessionId>, Timestamp)>,
    password_resets: Vec<PasswordReset>,
    new_roles: Vec<Role>,
    new_users: Vec<User>,
    invites: Vec<Invite>,
}

impl StageIdentities for InMemoryBatch {
    fn insert_identity(&mut self, identity: &mut Identity) {
        self.new_identities.push(identity.clone());
        identity.set_version(Version::NEW.next());
    }

    fn update_identity(&mut self, identity: &mut Identity) {
        self.identities.push(identity.clone());
        identity.set_version(identity.version().next());
    }
}

impl StageSessions for InMemoryBatch {
    fn insert_session(&mut self, session: &mut Session) {
        self.new_sessions.push(session.clone());
        session.set_version(Version::NEW.next());
    }

    fn update_session(&mut self, session: &mut Session) {
        self.sessions.push(session.clone());
        session.set_version(session.version().next());
    }

    fn revoke_sessions(&mut self, identity: IdentityId, keep: Option<SessionId>, at: Timestamp) {
        self.session_revocations.push((identity, keep, at));
    }
}

impl StagePasswordResets for InMemoryBatch {
    fn update_password_reset(&mut self, reset: &mut PasswordReset) {
        self.password_resets.push(reset.clone());
        reset.set_version(reset.version().next());
    }
}

impl StageRoles for InMemoryBatch {
    fn insert_role(&mut self, role: &mut Role) {
        self.new_roles.push(role.clone());
        role.set_version(Version::NEW.next());
    }
}

impl StageUsers for InMemoryBatch {
    fn insert_user(&mut self, user: &mut User) {
        self.new_users.push(user.clone());
        user.set_version(Version::NEW.next());
    }
}

impl StageInvites for InMemoryBatch {
    fn update_invite(&mut self, invite: &mut Invite) {
        self.invites.push(invite.clone());
        invite.set_version(invite.version().next());
    }
}

impl StageCarts for InMemoryBatch {
    fn insert_cart(&mut self, cart: &mut Cart) {
        self.new_carts.push(cart.clone());
        cart.set_version(Version::NEW.next());
    }

    fn update_cart(&mut self, cart: &mut Cart) {
        self.carts.push(cart.clone());
        cart.set_version(cart.version().next());
    }
}

impl StageInventoryItems for InMemoryBatch {
    fn update_inventory_item(&mut self, item: &mut InventoryItem) {
        self.inventory_items.push(item.clone());
        item.set_version(item.version().next());
    }
}

impl StageOrders for InMemoryBatch {
    fn insert_order(&mut self, order: &mut Order) {
        self.new_orders.push(order.clone());
        order.set_version(Version::NEW.next());
    }
}

impl StageCustomers for InMemoryBatch {
    fn insert_customer(&mut self, customer: &mut Customer) {
        self.new_customers.push(customer.clone());
        customer.set_version(Version::NEW.next());
    }
}

impl StageIdempotency for InMemoryBatch {
    fn record_idempotency(&mut self, record: IdempotencyRecord) {
        self.idempotency.push(record);
    }
}

impl HasBatch for InMemoryPorts {
    type Batch = InMemoryBatch;

    fn batch(&self) -> InMemoryBatch {
        InMemoryBatch::default()
    }

    /// Locks every table a batch can write, always in this order, so two commits can't
    /// deadlock and a single write (which locks one table) can't slip in between. The
    /// writes then run with the usual version and key checks; if one fails, the tables
    /// are put back as they were. Readers wait for the locks, so they never see half a
    /// batch. Copying the tables for the rollback is fine at test sizes.
    async fn commit(&self, batch: InMemoryBatch) -> Result<(), RepoError> {
        let mut orders = self.orders.orders.lock();
        let mut carts = self.carts.carts.lock();
        let mut items = self.inventory_items.items.lock();
        let mut customers = self.customers.customers.lock();
        let mut records = self.idempotency.lock();
        let mut identities = self.identities.identities.lock();
        let mut sessions = self.sessions.sessions.lock();
        let mut resets = self.password_resets.resets.lock();
        let mut roles = self.roles.roles.lock();
        let mut users = self.users.users.lock();
        let mut invites = self.invites.invites.lock();
        let before = (
            orders.clone(),
            carts.clone(),
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

        let written = (|| {
            for mut order in batch.new_orders {
                insert_row(
                    &mut orders,
                    order.id(),
                    &mut order,
                    order_keys::ORDER_ID,
                    &unique_order,
                )?;
            }
            for mut cart in batch.new_carts {
                insert_row(
                    &mut carts,
                    cart.id(),
                    &mut cart,
                    cart_keys::CART_ID,
                    &no_unique_keys,
                )?;
            }
            for mut cart in batch.carts {
                update_row(&mut carts, cart.id(), &mut cart, &no_unique_keys)?;
            }
            for mut item in batch.inventory_items {
                update_row(&mut items, item.id(), &mut item, &inventory_items::unique)?;
            }
            for mut customer in batch.new_customers {
                insert_row(
                    &mut customers,
                    customer.id(),
                    &mut customer,
                    customer_keys::CUSTOMER_ID,
                    &unique_customer,
                )?;
            }
            for record in batch.idempotency {
                insert_record(&mut records, record)?;
            }
            for mut role in batch.new_roles {
                insert_row(
                    &mut roles,
                    role.id(),
                    &mut role,
                    user_keys::ROLE_ID,
                    &unique_role,
                )?;
            }
            for mut user in batch.new_users {
                insert_row(
                    &mut users,
                    user.id(),
                    &mut user,
                    user_keys::USER_ID,
                    &unique_user,
                )?;
            }
            for mut invite in batch.invites {
                update_row(&mut invites, invite.id(), &mut invite, &unique_invite)?;
            }
            for mut identity in batch.new_identities {
                insert_row(
                    &mut identities,
                    identity.id(),
                    &mut identity,
                    auth_keys::IDENTITY_ID,
                    &unique_identity,
                )?;
            }
            for mut identity in batch.identities {
                update_row(
                    &mut identities,
                    identity.id(),
                    &mut identity,
                    &unique_identity,
                )?;
            }
            for mut session in batch.new_sessions {
                insert_row(
                    &mut sessions,
                    session.id(),
                    &mut session,
                    auth_keys::SESSION_ID,
                    &unique_session,
                )?;
            }
            for mut session in batch.sessions {
                update_row(&mut sessions, session.id(), &mut session, &unique_session)?;
            }
            for (identity, keep, at) in batch.session_revocations {
                for session in sessions.values_mut().filter(|s| {
                    s.identity() == identity && Some(s.id()) != keep && s.revoked_at().is_none()
                }) {
                    session.revoke(at);
                    session.set_version(session.version().next());
                }
            }
            for mut reset in batch.password_resets {
                update_row(&mut resets, reset.id(), &mut reset, &unique_reset)?;
            }
            Ok(())
        })();
        if written.is_err() {
            (*orders, *carts, *items, *customers, *records) = before;
            (*identities, *sessions, *resets, *roles, *users, *invites) = before_staff;
        }
        written
    }
}
