use std::sync::Arc;

use hypha_cart::CartPorts;
use hypha_core::IdGenerator;
use hypha_store_postgres::{CORE, PgConnection, PgStore, migrate};
use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo,
};
use uuid::Uuid;

use crate::error::ApiError;

/// Real IDs: UUIDv7 (time-ordered), generated from the system clock and RNG.
#[derive(Default)]
pub struct UuidV7Ids;

impl IdGenerator for UuidV7Ids {
    fn new_uuid(&self) -> Uuid {
        Uuid::now_v7()
    }
}

pub struct AppState<S> {
    pub storage: S,
    pub ids: UuidV7Ids,
}

pub type SharedState<S> = Arc<AppState<S>>;

/// Where the repositories come from. Handlers ask for `ports()` once per request and
/// pass them to commands; `CartPorts` gives access to every repository.
///
/// Handlers are generic over this, but axum can only prove their futures are `Send` for
/// a concrete storage type (the repo traits deliberately have no `Send` bound), so the
/// router is built per concrete type (see `routes::router!`).
#[allow(async_fn_in_trait)]
pub trait Storage: Send + Sync + 'static {
    type Ports<'a>: CartPorts
    where
        Self: 'a;

    async fn ports(&self) -> Result<Self::Ports<'_>, ApiError>;
}

// ---------- in memory (default; data is lost on restart) ----------

#[derive(Default)]
pub struct MemoryStorage {
    products: InMemoryProductRepo,
    markets: InMemoryMarketRepo,
    catalogs: InMemoryCatalogRepo,
    price_lists: InMemoryPriceListRepo,
    carts: InMemoryCartRepo,
    tax_classes: InMemoryTaxClassRepo,
    tax_regions: InMemoryTaxRegionRepo,
}

impl CartPorts for MemoryStorage {
    type Carts = InMemoryCartRepo;
    type Products = InMemoryProductRepo;
    type Markets = InMemoryMarketRepo;
    type Catalogs = InMemoryCatalogRepo;
    type PriceLists = InMemoryPriceListRepo;
    type TaxClasses = InMemoryTaxClassRepo;
    type TaxRegions = InMemoryTaxRegionRepo;

    fn carts(&self) -> &Self::Carts {
        &self.carts
    }
    fn products(&self) -> &Self::Products {
        &self.products
    }
    fn markets(&self) -> &Self::Markets {
        &self.markets
    }
    fn catalogs(&self) -> &Self::Catalogs {
        &self.catalogs
    }
    fn price_lists(&self) -> &Self::PriceLists {
        &self.price_lists
    }
    fn tax_classes(&self) -> &Self::TaxClasses {
        &self.tax_classes
    }
    fn tax_regions(&self) -> &Self::TaxRegions {
        &self.tax_regions
    }
}

impl Storage for MemoryStorage {
    type Ports<'a> = &'a MemoryStorage;

    async fn ports(&self) -> Result<&MemoryStorage, ApiError> {
        Ok(self)
    }
}

// ---------- Postgres (when DATABASE_URL is set) ----------

/// A connection checked out of the pool; it goes back when the request's store is dropped.
pub struct PooledClient(deadpool_postgres::Object);

impl PgConnection for PooledClient {
    fn client(&mut self) -> &mut tokio_postgres::Client {
        // Object -> deadpool's ClientWrapper -> tokio_postgres::Client
        &mut self.0
    }
}

pub struct PgStorage {
    pool: deadpool_postgres::Pool,
}

impl PgStorage {
    /// Opens a connection pool and applies pending migrations.
    pub async fn connect(url: &str) -> Result<Self, String> {
        let mut config = deadpool_postgres::Config::new();
        config.url = Some(url.to_string());
        let pool = config
            .create_pool(
                Some(deadpool_postgres::Runtime::Tokio1),
                tokio_postgres::NoTls,
            )
            .map_err(|e| format!("invalid DATABASE_URL: {e}"))?;
        let mut conn = pool
            .get()
            .await
            .map_err(|e| format!("cannot connect to Postgres: {e}"))?;
        migrate(&mut conn, &[CORE])
            .await
            .map_err(|e| format!("migrations failed: {e}"))?;
        Ok(Self { pool })
    }
}

impl Storage for PgStorage {
    type Ports<'a> = PgStore<PooledClient>;

    async fn ports(&self) -> Result<PgStore<PooledClient>, ApiError> {
        let conn = self.pool.get().await.map_err(ApiError::internal)?;
        Ok(PgStore::new(PooledClient(conn)))
    }
}
