use futures_util::lock::{Mutex, MutexGuard};
use hypha_core::RepoError;
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
}

impl<C: PgConnection> PgStore<C> {
    pub fn new(conn: C) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub(crate) async fn conn(&self) -> MutexGuard<'_, C> {
        self.conn.lock().await
    }
}

/// One `PgStore` implements every repository, so it is all of the cart's ports at once.
impl<C: PgConnection> hypha_cart::CartPorts for PgStore<C> {
    type Carts = Self;
    type Products = Self;
    type Markets = Self;
    type Catalogs = Self;
    type PriceLists = Self;
    type TaxClasses = Self;
    type TaxRegions = Self;
    type InventoryItems = Self;
    type Locations = Self;
    type Customers = Self;

    fn carts(&self) -> &Self {
        self
    }
    fn products(&self) -> &Self {
        self
    }
    fn markets(&self) -> &Self {
        self
    }
    fn catalogs(&self) -> &Self {
        self
    }
    fn price_lists(&self) -> &Self {
        self
    }
    fn tax_classes(&self) -> &Self {
        self
    }
    fn tax_regions(&self) -> &Self {
        self
    }
    fn inventory_items(&self) -> &Self {
        self
    }
    fn locations(&self) -> &Self {
        self
    }
    fn customers(&self) -> &Self {
        self
    }
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
        $crate::store::end_consistent_read(read, value).await
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

/// Commits the read if it succeeded; on error the transaction is dropped, which rolls
/// it back.
pub(crate) async fn end_consistent_read<T>(
    read: Transaction<'_>,
    value: Result<T, RepoError>,
) -> Result<T, RepoError> {
    let value = value?;
    read.commit().await.map_err(repo_error)?;
    Ok(value)
}
