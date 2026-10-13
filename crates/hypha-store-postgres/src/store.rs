use futures_util::lock::{Mutex, MutexGuard};
use tokio_postgres::Client;

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
