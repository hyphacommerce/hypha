use futures_util::lock::{Mutex, MutexGuard};
use std::sync::Arc;

use hypha_core::{Actor, Clock, RepoError, Timestamp};
use tokio_postgres::{Client, IsolationLevel, Transaction};

use crate::error::repo_error;

/// Something that holds a connected `tokio_postgres::Client`: the client itself, a
/// borrowed one, or (in a host) a connection checked out of a pool.
pub trait PgConnection {
    fn client(&mut self) -> &mut Client;
}

impl PgConnection for Client {
    fn client(&mut self) -> &mut Client {
        self
    }
}

impl PgConnection for &mut Client {
    fn client(&mut self) -> &mut Client {
        self
    }
}

/// Postgres implementation of every repository port.
///
/// The connection sits behind an async mutex, so one operation uses it at a time: a
/// transaction can't interleave with another query on the same connection, even if the
/// store is shared. Transactions need `&mut Client`, which the mutex hands out.
pub struct PgStore<C> {
    conn: Mutex<C>,
    /// Who the events this store writes are stamped with (`ActingAs`, design note 0016,
    /// Q1): `Actor::System` until the host sets the caller. A plain mutex: it's held
    /// only to copy an `Actor`, never across an `.await`.
    actor: std::sync::Mutex<Actor>,
    /// What aggregates are stamped with when written (design note 0016, Q3): the host's
    /// clock, so a batch's aggregates carry their stamps before it commits.
    clock: Arc<dyn Clock + Send + Sync>,
}

impl<C: PgConnection> PgStore<C> {
    pub fn new(conn: C, clock: Arc<dyn Clock + Send + Sync>) -> Self {
        Self {
            conn: Mutex::new(conn),
            actor: std::sync::Mutex::new(Actor::System),
            clock,
        }
    }

    /// The time writes are stamped with.
    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) async fn conn(&self) -> MutexGuard<'_, C> {
        self.conn.lock().await
    }

    /// The actor events are stamped with now.
    pub(crate) fn actor(&self) -> Actor {
        // A poisoned lock still holds a whole `Actor` (it's `Copy`; no write is torn).
        *self
            .actor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn set_actor(&self, actor: Actor) {
        *self
            .actor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = actor;
    }
}

/// One `PgStore` implements every repository, so each accessor returns the store itself
/// and it is all of the cart's ports (`hypha_cart::CartPorts`) at once.
macro_rules! accessors {
    ($($trait:path => $assoc:ident, $method:ident;)*) => {$(
        impl<C: PgConnection> $trait for PgStore<C> {
            type $assoc = Self;
            fn $method(&self) -> &Self {
                self
            }
        }
    )*};
}

accessors! {
    hypha_cart::HasCarts => Carts, carts;
    hypha_products::HasProducts => Products, products;
    hypha_markets::HasMarkets => Markets, markets;
    hypha_markets::HasCatalogs => Catalogs, catalogs;
    hypha_markets::HasPriceLists => PriceLists, price_lists;
    hypha_tax::HasTaxClasses => TaxClasses, tax_classes;
    hypha_tax::HasTaxRegions => TaxRegions, tax_regions;
    hypha_inventory::HasInventoryItems => InventoryItems, inventory_items;
    hypha_inventory::HasLocations => Locations, locations;
    hypha_customers::HasCustomers => Customers, customers;
    hypha_core::HasIdempotency => Idempotency, idempotency;
    hypha_orders::HasOrders => Orders, orders;
    hypha_fulfillment::HasFulfillments => Fulfillments, fulfillments;
    hypha_auth::HasIdentities => Identities, identities;
    hypha_auth::HasSessions => Sessions, sessions;
    hypha_auth::HasApiKeys => ApiKeys, api_keys;
    hypha_auth::HasPasswordResets => PasswordResets, password_resets;
    hypha_users::HasUsers => Users, users;
    hypha_users::HasRoles => Roles, roles;
    hypha_users::HasInvites => Invites, invites;
    hypha_media::HasAssets => Assets, assets;
    hypha_media::HasAssetUsages => AssetUsages, asset_usages;
    hypha_shipping::HasShippingZones => ShippingZones, shipping_zones;
    hypha_shipping::HasAppProviders => AppProviders, app_providers;
    hypha_core::HasOutbox => Outbox, outbox;
    hypha_apps::HasApps => Apps, apps;
    hypha_search_index::HasSearchQueue => SearchQueue, search_queue;
    hypha_search_index::HasSearchSettings => SearchSettings, search_settings;
    hypha_channels::HasChannels => Channels, channels;
    hypha_categories::HasCategories => Categories, categories;
}

/// Runs a load that takes several queries (a root row, then its children) in one
/// consistent snapshot: `REPEATABLE READ READ ONLY`. Without it, a write committing
/// between the queries could pair a new root with old children, `restore` would reject
/// the mix, and the reader would get a 500 (review plan B3). One query is consistent on
/// its own; single-query reads don't need this.
///
/// `consistent_read!(client, |tx| body)` runs `body` (which returns
/// `Result<_, RepoError>`) with `tx: &Transaction`, then commits. A macro rather than a
/// function taking an async closure: the compiler can't yet prove such futures `Send`
/// ("`AsyncFnOnce` is not general enough"), and hosts like axum need them `Send`.
macro_rules! consistent_read {
    ($client:expr, |$tx:ident| $body:expr) => {{
        let read = $crate::store::begin_consistent_read($client).await?;
        let $tx = &read;
        let value: Result<_, hypha_core::RepoError> = async { $body }.await;
        $crate::store::end_transaction(read, value).await
    }};
}
pub(crate) use consistent_read;

pub(crate) async fn begin_consistent_read(
    client: &mut Client,
) -> Result<Transaction<'_>, RepoError> {
    client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(repo_error)
}

/// Runs a repository write as a batch of one (design note 0016, EV2): `body` (which
/// returns `Result<_, RepoError>`) gets `tx: &Transaction` and calls the same
/// `insert_x` / `update_x` functions `PgBatch::commit` does, then the transaction
/// commits. On an error the transaction is dropped, which rolls it back, so a write
/// that fails half-way (a stale version after some child rows) leaves nothing behind.
/// A macro for the same reason as `consistent_read!`.
///
/// `in_transaction!(store, |tx| body)`.
macro_rules! in_transaction {
    ($store:expr, |$tx:ident| $body:expr) => {{
        let mut conn = $store.conn().await;
        let transaction = conn
            .client()
            .transaction()
            .await
            .map_err($crate::error::repo_error)?;
        let $tx = &transaction;
        let value: Result<_, hypha_core::RepoError> = async { $body }.await;
        $crate::store::end_transaction(transaction, value).await
    }};
}
pub(crate) use in_transaction;

/// Commits a read or write if it succeeded; on error the transaction is dropped, which
/// rolls it back.
pub(crate) async fn end_transaction<T>(
    tx: Transaction<'_>,
    value: Result<T, RepoError>,
) -> Result<T, RepoError> {
    let value = value?;
    tx.commit().await.map_err(repo_error)?;
    Ok(value)
}
