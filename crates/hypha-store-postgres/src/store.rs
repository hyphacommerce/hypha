use futures_util::lock::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use hypha_core::{
    Actor, Clock, DispatchHook, PreviewToken, RepoError, RowChange, StoredEvent, Timestamp,
    WriteRecord, WrittenRow,
};
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

    /// Whether a read-only snapshot that succeeded may end without waiting
    /// (`consistent_read!`): its ROLLBACK is queued, and the next query on the connection
    /// follows it, which saves a round trip per load. `false`, the default, waits: through
    /// Hyperdrive a transaction ended without waiting stalled the next query. A host's
    /// direct connection says `true`.
    fn ends_reads_unawaited(&self) -> bool {
        false
    }

    /// Told when a preview's transaction opens on this connection (`open`) and when it's
    /// ended. A preview whose request was dropped half-way (cancelled, panicked) never
    /// ends, and holds its locks until its connection closes, so a pooled connection
    /// dropped with one open must be closed rather than go back to the pool; one that
    /// closes when dropped (a Workers request's, a test's) needs nothing, the default.
    fn preview_open(&mut self, open: bool) {
        let _ = open;
    }

    /// Queries on the client outside a transaction, through its statement cache.
    fn db(&mut self) -> Db<'_, Client> {
        let (client, statements) = self.parts();
        Db::new(client, statements)
    }
}

/// The statements one connection has prepared, by SQL and parameter types. An unprepared
/// query is planned on every call, and planning cost Postgres about four times what running
/// the storefront's queries did; a prepared one is planned once per connection. Kept with
/// the connection (a statement belongs to it), so only a host with long-lived connections
/// keeps one.
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
    /// Inside a preview, outside any write's savepoint (`StoreConn::db`): each statement
    /// runs in a savepoint of its own, so one that fails (a unique key, a reference)
    /// fails alone, as it would on its own connection, instead of aborting the preview's
    /// whole transaction.
    savepoint_each: bool,
}

impl<'a, G> Db<'a, G> {
    pub fn new(client: &'a G, statements: Option<&'a StatementCache>) -> Self {
        Self {
            client,
            statements,
            savepoint_each: false,
        }
    }
}

/// The savepoint each statement takes inside a preview (`Db::savepoint_each`).
const STATEMENT_SAVEPOINT: &str = "hypha_statement";

impl<G: GenericClient + Sync> Db<'_, G> {
    /// Runs `statement`, in a savepoint of its own when `savepoint_each` says so.
    async fn run<T>(
        &self,
        statement: impl std::future::Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        if !self.savepoint_each {
            return statement.await;
        }
        self.client
            .batch_execute(&format!("SAVEPOINT {STATEMENT_SAVEPOINT}"))
            .await?;
        let result = statement.await;
        let end = match result {
            Ok(_) => format!("RELEASE SAVEPOINT {STATEMENT_SAVEPOINT}"),
            Err(_) => format!(
                "ROLLBACK TO SAVEPOINT {STATEMENT_SAVEPOINT}; RELEASE SAVEPOINT {STATEMENT_SAVEPOINT}"
            ),
        };
        // The statement's own error is the one to report; a failed end means the
        // connection is gone, which the next statement reports.
        let ended = self.client.batch_execute(&end).await;
        let value = result?;
        ended?;
        Ok(value)
    }

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
        self.run(async {
            match self.prepared(sql, params).await? {
                Some(statement) => self.client.query(&statement, &values(params)).await,
                None => self.client.query_typed(sql, params).await,
            }
        })
        .await
    }

    async fn query_typed_opt(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Option<Row>, Error> {
        self.run(async {
            match self.prepared(sql, params).await? {
                Some(statement) => self.client.query_opt(&statement, &values(params)).await,
                None => self.client.query_typed_opt(sql, params).await,
            }
        })
        .await
    }

    async fn query_typed_one(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<Row, Error> {
        self.run(async {
            match self.prepared(sql, params).await? {
                Some(statement) => self.client.query_one(&statement, &values(params)).await,
                None => self.client.query_typed_one(sql, params).await,
            }
        })
        .await
    }

    async fn execute_typed(
        &self,
        sql: &str,
        params: &[(&(dyn ToSql + Sync), Type)],
    ) -> Result<u64, Error> {
        self.run(async {
            match self.prepared(sql, params).await? {
                Some(statement) => self.client.execute(&statement, &values(params)).await,
                None => self.client.execute_typed(sql, params).await,
            }
        })
        .await
    }

    async fn batch_execute(&self, sql: &str) -> Result<(), Error> {
        self.run(self.client.batch_execute(sql)).await
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
/// store is shared. Transactions need `&mut Client`, which the mutex hands out. The mutex
/// is shared (`Arc`) so an open preview (`PgPreview`) can end on the connection after the
/// store that began it is gone.
pub struct PgStore<C> {
    conn: Arc<Mutex<C>>,
    /// Who the events this store writes are stamped with (`ActingAs`): `Actor::System`
    /// until the host sets the caller. A plain mutex: it's held only to copy an `Actor`,
    /// never across an `.await`.
    actor: std::sync::Mutex<Actor>,
    /// What aggregates are stamped with when written: the host's clock, so a batch's
    /// aggregates carry their stamps before it commits.
    clock: Arc<dyn Clock + Send + Sync>,
    /// Whether the open transaction has given the dispatcher work (`insert_events`, a
    /// retried delivery): when it commits, the dispatch hook is told. Transactions don't overlap on one store (the
    /// connection's mutex), so one flag is enough.
    events_pending: AtomicBool,
    /// Told after a write gives the dispatcher work (`SignalsDispatch`); none until the
    /// host sets one. A plain mutex, held only to clone the `Arc`.
    dispatch_hook: std::sync::Mutex<Option<DispatchHook>>,
    /// Set once the store runs in a preview (`begin_preview`): what its writes recorded.
    preview: OnceLock<Arc<PreviewLog>>,
}

impl<C: PgConnection> PgStore<C> {
    pub fn new(conn: C, clock: Arc<dyn Clock + Send + Sync>) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            actor: std::sync::Mutex::new(Actor::System),
            clock,
            events_pending: AtomicBool::new(false),
            dispatch_hook: std::sync::Mutex::new(None),
            preview: OnceLock::new(),
        }
    }

    /// The time writes are stamped with.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// The request's connection, for a query: `conn.db()` (`StoreConn::db`) queries
    /// outside a transaction through its statement cache. Public for spores' own
    /// repositories over their own tables, which implement their accessor traits for
    /// `PgStore`.
    pub async fn conn(&self) -> StoreConn<'_, C> {
        StoreConn {
            guard: self.conn.lock().await,
            preview: self.in_preview(),
        }
    }

    /// Whether this store runs in a preview (`begin_preview`).
    pub fn in_preview(&self) -> bool {
        self.preview.get().is_some()
    }

    /// Runs everything this store does from now on in one transaction that's never
    /// committed: a preview. Each write's transaction becomes a savepoint inside it, so a
    /// write that fails is rolled back to where it began, as it would be on its own, and
    /// what it wrote stays visible to the reads after it. Every statement is cut off
    /// after `timeout`, which bounds the locks a preview holds.
    ///
    /// The `PgPreview` ends it: what the writes recorded, then the ROLLBACK. Dropped
    /// unended (the request was cancelled), the connection is told
    /// (`PgConnection::preview_open`), and Postgres ends a transaction left idle past
    /// `PREVIEW_LEFT_OPEN`.
    pub async fn begin_preview(&self, timeout: Duration) -> Result<PgPreview<C>, RepoError> {
        let millis = timeout.as_millis().max(1);
        self.begin_outer(&format!("SET LOCAL statement_timeout = {millis}; "))
            .await
    }

    /// Runs everything this store does from now on in one transaction, as a preview does,
    /// which `PgPreview::finish` commits or rolls back as a whole once it has read what the
    /// writes recorded: applying a previewed write, which commits only if it recorded what
    /// the preview did. Statements aren't cut off (it's a real write), and the dispatch
    /// hook (`SignalsDispatch`, set before this) is told after the COMMIT.
    pub async fn begin_apply(&self) -> Result<PgPreview<C>, RepoError> {
        self.begin_outer("").await
    }

    async fn begin_outer(&self, settings: &str) -> Result<PgPreview<C>, RepoError> {
        let log = Arc::new(PreviewLog::default());
        if self.preview.set(Arc::clone(&log)).is_err() {
            return Err(RepoError::Backend(
                "the store is already in a preview".into(),
            ));
        }
        let hook = self
            .dispatch_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let mut conn = self.conn.lock().await;
        conn.preview_open(true);
        let left_open = PREVIEW_LEFT_OPEN.as_millis();
        conn.client()
            .batch_execute(&format!(
                "BEGIN; {settings}SET LOCAL idle_in_transaction_session_timeout = {left_open}; \
                 SET LOCAL hypha.log_writes = 'on'"
            ))
            .await
            .map_err(repo_error)?;
        Ok(PgPreview {
            conn: Arc::clone(&self.conn),
            log,
            hook,
        })
    }

    /// What `insert_events` needs from the store: the actor to stamp events with, and
    /// the flag that tells the dispatch hook once their transaction commits.
    pub(crate) fn event_stamp(&self) -> EventStamp<'_> {
        EventStamp {
            actor: self.actor(),
            pending: &self.events_pending,
            preview: self.preview.get().map(Arc::as_ref),
        }
    }

    /// Ends a write transaction's bookkeeping: if it `committed` events, the dispatch
    /// hook is told now (after the COMMIT, so the dispatcher finds them).
    pub(crate) fn transaction_ended(&self, committed: bool) {
        if self.events_pending.swap(false, Ordering::Relaxed) && committed {
            self.dispatch_work();
        }
    }

    /// Tells the dispatch hook, if there is one, that the dispatcher has work. Never in a
    /// preview: nothing it writes is committed.
    pub(crate) fn dispatch_work(&self) {
        if self.in_preview() {
            return;
        }
        let hook = self
            .dispatch_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook();
        }
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

/// How long Postgres lets a preview's transaction sit idle before it ends the session:
/// the bound on the locks of a preview whose request was dropped before it rolled back.
/// Longer than any call a preview waits on mid-transaction (a read-only extension point).
pub const PREVIEW_LEFT_OPEN: Duration = Duration::from_secs(60);

/// What a preview's writes recorded: the outbox rows' `seq`s, in order. A write rolled back
/// to its savepoint leaves its numbers here, but not its rows, so `PgPreview::events` reads
/// back what's still there.
#[derive(Default)]
pub(crate) struct PreviewLog {
    events: std::sync::Mutex<Vec<i64>>,
}

impl PreviewLog {
    fn wrote(&self, seqs: impl IntoIterator<Item = i64>) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(seqs);
    }

    fn written(&self) -> Vec<i64> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// An open preview (`PgStore::begin_preview`) or apply (`begin_apply`), on the connection
/// it shares with its store. Ended in two steps: `events` reads what the writes recorded,
/// then `finish` commits or rolls back.
pub struct PgPreview<C> {
    conn: Arc<Mutex<C>>,
    log: Arc<PreviewLog>,
    /// Told after a COMMIT that wrote events.
    hook: Option<DispatchHook>,
}

impl<C> Clone for PgPreview<C> {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            log: Arc::clone(&self.log),
            hook: self.hook.clone(),
        }
    }
}

impl<C: PgConnection> PgPreview<C> {
    /// The events its writes recorded and kept, in the order written, as the outbox
    /// stores them. Waits for the store's last query, if it's still running one.
    pub async fn events(&self) -> Result<Vec<StoredEvent>, RepoError> {
        let mut conn = self.conn.lock().await;
        let seqs = self.log.written();
        let (client, statements) = conn.parts();
        crate::outbox::events_by_seq(&Db::new(&*client, statements), &seqs).await
    }

    /// The rows of aggregates its writes wrote and kept, in the order written, each with the
    /// version it found (`hypha_note_write`, migration 0073): read from the transaction's
    /// own log, so a write rolled back to its savepoint isn't there.
    pub async fn rows(&self) -> Result<Vec<WrittenRow>, RepoError> {
        let mut conn = self.conn.lock().await;
        let (client, statements) = conn.parts();
        let row = Db::new(&*client, statements)
            .query_typed_one(
                "SELECT current_setting('hypha.writes', true) AS writes",
                &[],
            )
            .await
            .map_err(repo_error)?;
        let log: Option<String> = crate::columns::col(&row, "writes")?;
        log.unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .map(written_row)
            .collect()
    }

    /// What its writes did: `events`, then `rows`.
    pub async fn recorded(&self) -> Result<WriteRecord, RepoError> {
        Ok(WriteRecord {
            events: self.events().await?,
            rows: self.rows().await?,
        })
    }

    /// Ends it: everything committed (`commit`) or rolled back, then `token` stored, on
    /// its own, outside the transaction. A preview passes `false`, an apply `true` when its
    /// events matched.
    pub async fn finish(self, commit: bool, token: Option<PreviewToken>) -> Result<(), RepoError> {
        let mut conn = self.conn.lock().await;
        let ended = conn
            .client()
            .batch_execute(if commit { "COMMIT" } else { "ROLLBACK" })
            .await;
        conn.preview_open(false);
        ended.map_err(repo_error)?;
        if commit
            && !self.log.written().is_empty()
            && let Some(hook) = &self.hook
        {
            hook();
        }
        if let Some(token) = token {
            let (client, statements) = conn.parts();
            crate::preview_tokens::insert_token(&Db::new(&*client, statements), &token).await?;
        }
        Ok(())
    }
}

/// One line of the write log: `["<table>", "<key>", "insert"|"update"|"delete", <version
/// found or null>]`.
fn written_row(line: &str) -> Result<WrittenRow, RepoError> {
    let corrupt = |why: &str| RepoError::Backend(format!("the write log {line:?}: {why}"));
    let (kind, key, op, found): (String, String, String, Option<i64>) =
        serde_json::from_str(line).map_err(|e| corrupt(&e.to_string()))?;
    let change = match (op.as_str(), found) {
        ("insert", None) => RowChange::Inserted,
        ("update", Some(v)) => RowChange::Updated(v),
        ("delete", Some(v)) => RowChange::Deleted(v),
        _ => return Err(corrupt("not a write")),
    };
    Ok(WrittenRow { kind, key, change })
}

/// The store's connection while it's locked (`PgStore::conn`): the host's connection
/// (`Deref`), and whether the store runs in a preview.
pub struct StoreConn<'a, C> {
    guard: MutexGuard<'a, C>,
    preview: bool,
}

impl<C: PgConnection> StoreConn<'_, C> {
    /// Queries on the client outside a transaction, through its statement cache; in a
    /// preview, each in a savepoint of its own (`Db`).
    pub fn db(&mut self) -> Db<'_, Client> {
        let preview = self.preview;
        let (client, statements) = self.guard.parts();
        Db {
            client: &*client,
            statements,
            savepoint_each: preview,
        }
    }

    /// Whether the store runs in a preview: transactions are savepoints then.
    pub(crate) fn in_preview(&self) -> bool {
        self.preview
    }
}

impl<C> Deref for StoreConn<'_, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.guard
    }
}

impl<C> DerefMut for StoreConn<'_, C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.guard
    }
}

impl<C: PgConnection> hypha_core::SignalsDispatch for PgStore<C> {
    fn on_dispatch_work(&self, hook: DispatchHook) {
        *self
            .dispatch_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    }
}

/// The actor `insert_events` stamps events with, and where it notes that it wrote some
/// (`PgStore::event_stamp`).
#[derive(Clone, Copy)]
pub(crate) struct EventStamp<'a> {
    pub actor: Actor,
    pending: &'a AtomicBool,
    /// In a preview, where the rows written are noted.
    preview: Option<&'a PreviewLog>,
}

impl EventStamp<'_> {
    /// The open transaction gave the dispatcher work: events, or a delivery due again.
    pub(crate) fn wrote(&self) {
        self.pending.store(true, Ordering::Relaxed);
    }

    /// Whether the store runs in a preview or an apply, which reads back what it wrote.
    pub(crate) fn in_preview(&self) -> bool {
        self.preview.is_some()
    }

    /// The outbox rows just written, by `seq`, for a preview to read back.
    pub(crate) fn wrote_events(&self, seqs: impl IntoIterator<Item = i64>) {
        if let Some(log) = self.preview {
            log.wrote(seqs);
        }
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

impl<C: PgConnection> hypha_core::HasClock for PgStore<C> {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }
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
    hypha_customers::HasCustomerGroups => CustomerGroups, customer_groups;
    hypha_core::HasIdempotency => Idempotency, idempotency;
    hypha_orders::HasOrders => Orders, orders;
    hypha_fulfillment::HasFulfillments => Fulfillments, fulfillments;
    hypha_payments::HasPaymentMethods => PaymentMethods, payment_methods;
    hypha_payments::HasPayments => Payments, payments;
    hypha_auth::HasIdentities => Identities, identities;
    hypha_auth::HasSessions => Sessions, sessions;
    hypha_auth::HasApiKeys => ApiKeys, api_keys;
    hypha_auth::HasPasswordResets => PasswordResets, password_resets;
    hypha_users::HasUsers => Users, users;
    hypha_users::HasRoles => Roles, roles;
    hypha_users::HasInvites => Invites, invites;
    hypha_users::HasStaffSessions => StaffSessions, staff_sessions;
    hypha_media::HasAssets => Assets, assets;
    hypha_media::HasAssetUsages => AssetUsages, asset_usages;
    hypha_shipping::HasShippingZones => ShippingZones, shipping_zones;
    hypha_shipping::HasAppProviders => AppProviders, app_providers;
    hypha_core::HasOutbox => Outbox, outbox;
    hypha_apps::HasApps => Apps, apps;
    hypha_oauth::HasOAuthRequests => OAuthRequests, oauth_requests;
    hypha_oauth::HasOAuthCodes => OAuthCodes, oauth_codes;
    hypha_oauth::HasOAuthTokens => OAuthTokens, oauth_tokens;
    hypha_search_index::HasSearchQueue => SearchQueue, search_queue;
    hypha_search_index::HasSearchSettings => SearchSettings, search_settings;
    hypha_channels::HasChannels => Channels, channels;
    hypha_categories::HasCategories => Categories, categories;
    hypha_notifications::HasNotificationTemplates => NotificationTemplates, notification_templates;
    hypha_notifications::HasNotificationSettings => NotificationSettings, notification_settings;
    hypha_notifications::HasNotifications => Notifications, notifications;
    hypha_extensions::HasExtensionCalls => ExtensionCalls, extension_calls;
    hypha_apps::HasExtensionEndpoints => ExtensionEndpoints, extension_endpoints;
    hypha_apps::HasSporeSettings => SporeSettings, spore_settings;
    hypha_custom_fields::HasCustomFieldDefinitions => CustomFieldDefinitions, custom_field_definitions;
    hypha_custom_fields::HasCustomFieldValues => CustomFieldValues, custom_field_values;
    hypha_agents::HasAgents => Agents, agents;
    hypha_agents::HasProposals => Proposals, proposals;
    hypha_agents::HasAgentCalls => AgentCalls, agent_calls;
    hypha_core::HasPreviewTokens => PreviewTokens, preview_tokens;
}

/// Runs a load that takes several queries (a root row, then its children) in one
/// consistent snapshot: `REPEATABLE READ READ ONLY`. Without it, a write committing
/// between the queries could pair a new root with old children, `restore` would reject
/// the mix, and the reader would get a 500 (review plan B3). One query is consistent on
/// its own; single-query reads don't need this. In a preview the snapshot can't be
/// taken (the isolation of a transaction is fixed by its first query): the load reads the
/// preview's own view, in a savepoint.
///
/// `consistent_read!(conn, |tx| body)` runs `body` (which returns `Result<_, RepoError>`)
/// with `tx: &Db<Client>` (queries through the connection's statement cache), then
/// ends the snapshot. A macro rather than a
/// function taking an async closure: the compiler can't yet prove such futures `Send`
/// ("`AsyncFnOnce` is not general enough"), and hosts like axum need them `Send`.
macro_rules! consistent_read {
    ($conn:expr, |$tx:ident| $body:expr) => {{
        let unawaited = $crate::store::PgConnection::ends_reads_unawaited(&*$conn);
        let preview = $conn.in_preview();
        let (client, statements) = $crate::store::PgConnection::parts(&mut *$conn);
        let read = $crate::store::begin_consistent_read(client, preview).await?;
        let db = $crate::store::Db::new(read.client(), statements);
        let $tx = &db;
        let value: Result<_, hypha_core::RepoError> = async { $body }.await;
        $crate::store::end_read(read, value, unawaited).await
    }};
}
pub(crate) use consistent_read;

/// The savepoint a write takes inside a preview, in place of its transaction.
const WRITE_SAVEPOINT: &str = "hypha_write";

/// A write's or a snapshot's transaction: its own, or, inside a preview, a savepoint in
/// the preview's transaction, released or rolled back to like a transaction is committed
/// or rolled back.
pub(crate) enum Tx<'a> {
    Own(Transaction<'a>),
    Savepoint(&'a Client),
}

impl<'a> Tx<'a> {
    /// A write's transaction: in a preview (`preview`), a savepoint.
    pub(crate) async fn begin(client: &'a mut Client, preview: bool) -> Result<Self, RepoError> {
        if preview {
            return Self::savepoint(client).await;
        }
        let tx = client.transaction().await.map_err(repo_error)?;
        Ok(Tx::Own(tx))
    }

    async fn savepoint(client: &'a mut Client) -> Result<Self, RepoError> {
        client
            .batch_execute(&format!("SAVEPOINT {WRITE_SAVEPOINT}"))
            .await
            .map_err(repo_error)?;
        Ok(Tx::Savepoint(client))
    }

    /// The client its queries go through.
    pub(crate) fn client(&self) -> &Client {
        match self {
            Tx::Own(tx) => tx.client(),
            Tx::Savepoint(client) => client,
        }
    }

    async fn commit(self) -> Result<(), Error> {
        match self {
            Tx::Own(tx) => tx.commit().await,
            Tx::Savepoint(client) => {
                client
                    .batch_execute(&format!("RELEASE SAVEPOINT {WRITE_SAVEPOINT}"))
                    .await
            }
        }
    }

    async fn rollback(self) -> Result<(), Error> {
        match self {
            Tx::Own(tx) => tx.rollback().await,
            Tx::Savepoint(client) => {
                client
                    .batch_execute(&format!(
                        "ROLLBACK TO SAVEPOINT {WRITE_SAVEPOINT}; \
                         RELEASE SAVEPOINT {WRITE_SAVEPOINT}"
                    ))
                    .await
            }
        }
    }
}

pub(crate) async fn begin_consistent_read(
    client: &mut Client,
    preview: bool,
) -> Result<Tx<'_>, RepoError> {
    if preview {
        return Tx::savepoint(client).await;
    }
    let read = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(repo_error)?;
    Ok(Tx::Own(read))
}

/// Runs a repository write as a batch of one: `body` (which returns `Result<_, RepoError>`)
/// gets `tx: &Db<Client>` and calls the same `insert_x` / `update_x` functions
/// `PgBatch::commit` does, then the transaction commits. On an error it's rolled back
/// (`end_transaction`, awaited: never by dropping it), so a write that fails half-way (a
/// stale version after some child rows) leaves nothing behind. A commit that wrote events
/// tells the dispatch hook (`PgStore::transaction_ended`). In a preview, the transaction
/// is a savepoint (`Tx`). A macro for the same reason as `consistent_read!`.
///
/// `in_transaction!(store, |tx| body)`.
macro_rules! in_transaction {
    ($store:expr, |$tx:ident| $body:expr) => {{
        let mut conn = $store.conn().await;
        let preview = conn.in_preview();
        let (client, statements) = $crate::store::PgConnection::parts(&mut *conn);
        let transaction = $crate::store::Tx::begin(client, preview).await?;
        let db = $crate::store::Db::new(transaction.client(), statements);
        let $tx = &db;
        let value: Result<_, hypha_core::RepoError> = async { $body }.await;
        let ended = $crate::store::end_transaction(transaction, value).await;
        $store.transaction_ended(ended.is_ok());
        ended
    }};
}
pub(crate) use in_transaction;

/// Ends a read-only snapshot: like `end_transaction`, or, when the connection allows it
/// (`PgConnection::ends_reads_unawaited`) and the read succeeded, by dropping it, which
/// queues its ROLLBACK without waiting (nothing was written, so ending it either way is
/// the same). A preview's savepoint is always released, awaited.
pub(crate) async fn end_read<T>(
    read: Tx<'_>,
    value: Result<T, RepoError>,
    unawaited: bool,
) -> Result<T, RepoError> {
    match (read, value) {
        (Tx::Own(read), Ok(value)) if unawaited => {
            drop(read);
            Ok(value)
        }
        (read, value) => end_transaction(read, value).await,
    }
}

/// Ends a transaction explicitly, waiting for it: COMMIT if `value` succeeded, else
/// ROLLBACK (which a failed write needs anyway). Never by dropping it: tokio-postgres then
/// queues a ROLLBACK without waiting, which works against Postgres directly but stalled the
/// next query through Hyperdrive.
pub(crate) async fn end_transaction<T>(
    tx: Tx<'_>,
    value: Result<T, RepoError>,
) -> Result<T, RepoError> {
    match value {
        Ok(value) => {
            tx.commit().await.map_err(repo_error)?;
            Ok(value)
        }
        Err(e) => {
            // The error the caller needs is `e`; a failed rollback means the connection
            // is gone, which the next query will report.
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}
