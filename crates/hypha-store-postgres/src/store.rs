use futures_util::lock::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::Arc;

use hypha_core::{Actor, Clock, RepoError, Timestamp};
use tokio_postgres::{
    Client, Error, GenericClient, IsolationLevel, Row, Statement, Transaction,
    types::{ToSql, Type},
};

use crate::error::repo_error;

/// Something that holds a connected `tokio_postgres::Client`: the client itself, a
/// borrowed one, or (in a host) a connection checked out of a pool.
pub trait PgConnection {
    fn client(&mut self) -> &mut Client;

    /// The client with the statements its connection has prepared (`StatementCache`).
    /// `None`, the default, sends every query unprepared: right for a connection used
    /// once (tests, a Workers request), wrong for a pooled one, whose statements would
    /// be planned again on every call.
    fn parts(&mut self) -> (&mut Client, Option<&StatementCache>) {
        (self.client(), None)
    }

    /// Queries on the client outside a transaction, through its statement cache.
    fn db(&mut self) -> Db<'_, Client> {
        let (client, statements) = self.parts();
        Db::new(client, statements)
    }
}

/// The statements one connection has prepared, by SQL and parameter types (design note
/// 0025, §6). An unprepared query is planned on every call, and planning cost Postgres
/// about four times what running the storefront's queries did; a prepared one is
/// planned once per connection. Kept with the connection (a statement belongs to it),
/// so only a host with long-lived connections keeps one.
///
/// At most `CAPACITY` statements: past it a query runs unprepared, so SQL built per
/// call (a batch insert's rows) can't grow it without bound.
#[derive(Default)]
pub struct StatementCache {
    statements: std::sync::Mutex<Prepared>,
}

/// By SQL, each set of parameter types it was prepared with and the statement.
type Prepared = HashMap<String, Vec<(Vec<Type>, Statement)>>;

impl StatementCache {
    const CAPACITY: usize = 512;

    /// `sql` prepared on `client`'s connection, from the cache or now; `None` when the
    /// cache is full and it isn't in it.
    async fn prepare(
        &self,
        client: &impl GenericClient,
        sql: &str,
        types: &[Type],
    ) -> Result<Option<Statement>, Error> {
        {
            let statements = self.lock();
            if let Some(found) = statements
                .get(sql)
                .and_then(|kinds| kinds.iter().find(|(t, _)| t.as_slice() == types))
            {
                return Ok(Some(found.1.clone()));
            }
            if statements.len() >= Self::CAPACITY {
                return Ok(None);
            }
        }
        let statement = client.prepare_typed(sql, types).await?;
        self.lock()
            .entry(sql.to_string())
            .or_default()
            .push((types.to_vec(), statement.clone()));
        Ok(Some(statement))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Prepared> {
        // A poisoned lock still holds whole statements; nothing is torn by a panic.
        self.statements
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The queries the repositories run, with `tokio_postgres`'s names and arguments, on a
/// client or a transaction (`Db`), so the SQL at each call stays as written.
#[allow(async_fn_in_trait)]
pub trait Queries {
    async fn query_typed(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Vec<Row>, Error>;

    async fn query_typed_opt(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Option<Row>, Error>;

    async fn query_typed_one(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Row, Error>;

    async fn execute_typed(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<u64, Error>;

    async fn batch_execute(&self, sql: &str) -> Result<(), Error>;
}

/// A client or transaction with its connection's statement cache: each query runs as
/// the cached prepared statement, or unprepared without a cache (or a full one).
pub struct Db<'a, G> {
    client: &'a G,
    statements: Option<&'a StatementCache>,
}

impl<'a, G> Db<'a, G> {
    pub fn new(client: &'a G, statements: Option<&'a StatementCache>) -> Self {
        Self { client, statements }
    }
}

impl<G: GenericClient + Sync> Db<'_, G> {
    async fn prepared(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Option<Statement>, Error> {
        match self.statements {
            Some(cache) => {
                let types: Vec<Type> = params.iter().map(|(_, t)| t.clone()).collect();
                cache.prepare(self.client, sql, &types).await
            }
            None => Ok(None),
        }
    }
}

fn values<'p>(params: &'p [(&'p (dyn ToSql + Sync), Type)]) -> Vec<&'p (dyn ToSql + Sync)> {
    params.iter().map(|(v, _)| *v).collect()
}

impl<G: GenericClient + Sync> Queries for Db<'_, G> {
    async fn query_typed(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Vec<Row>, Error> {
        match self.prepared(sql, params).await? {
            Some(statement) => self.client.query(&statement, &values(params)).await,
            None => self.client.query_typed(sql, params).await,
        }
    }

    async fn query_typed_opt(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Option<Row>, Error> {
        match self.prepared(sql, params).await? {
            Some(statement) => self.client.query_opt(&statement, &values(params)).await,
            None => self.client.query_typed_opt(sql, params).await,
        }
    }

    async fn query_typed_one(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Row, Error> {
        match self.prepared(sql, params).await? {
            Some(statement) => self.client.query_one(&statement, &values(params)).await,
            None => self.client.query_typed_one(sql, params).await,
        }
    }

    async fn execute_typed(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<u64, Error> {
        match self.prepared(sql, params).await? {
            Some(statement) => self.client.execute(&statement, &values(params)).await,
            None => self.client.execute_typed(sql, params).await,
        }
    }

    async fn batch_execute(&self, sql: &str) -> Result<(), Error> {
        self.client.batch_execute(sql).await
    }
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
/// `consistent_read!(conn, |tx| body)` runs `body` (which returns `Result<_, RepoError>`)
/// with `tx: &Db<Transaction>` (queries through the connection's statement cache), then
/// ends the snapshot. A macro rather than a
/// function taking an async closure: the compiler can't yet prove such futures `Send`
/// ("`AsyncFnOnce` is not general enough"), and hosts like axum need them `Send`.
macro_rules! consistent_read {
    ($conn:expr, |$tx:ident| $body:expr) => {{
        let (client, statements) = $crate::store::PgConnection::parts(&mut *$conn);
        let read = $crate::store::begin_consistent_read(client).await?;
        let db = $crate::store::Db::new(&read, statements);
        let $tx = &db;
        let value: Result<_, hypha_core::RepoError> = async { $body }.await;
        $crate::store::end_read(read);
        value
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
        let (client, statements) = $crate::store::PgConnection::parts(&mut *conn);
        let transaction = client
            .transaction()
            .await
            .map_err($crate::error::repo_error)?;
        let db = $crate::store::Db::new(&transaction, statements);
        let $tx = &db;
        let value: Result<_, hypha_core::RepoError> = async { $body }.await;
        $crate::store::end_transaction(transaction, value).await
    }};
}
pub(crate) use in_transaction;

/// Ends a read-only snapshot without waiting for it: dropping the transaction queues
/// its ROLLBACK on the connection (tokio-postgres's `Drop`), which goes out with the
/// next query instead of costing a round trip of its own. Nothing was written, so
/// rolling back reads the same as committing; the connection is back to normal before
/// anything after it runs.
pub(crate) fn end_read(read: Transaction<'_>) {
    drop(read);
}

/// Commits a write if it succeeded; on error the transaction is dropped, which
/// rolls it back.
pub(crate) async fn end_transaction<T>(
    tx: Transaction<'_>,
    value: Result<T, RepoError>,
) -> Result<T, RepoError> {
    let value = value?;
    tx.commit().await.map_err(repo_error)?;
    Ok(value)
}
