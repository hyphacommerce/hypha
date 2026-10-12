use std::sync::Arc;

use hypha_cart::CartPorts;
use hypha_core::{IdGenerator, StoreId};
use hypha_store_postgres::{CORE, PgConnection, PgStore, migrate};
use hypha_stores::StoreRepo;
use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryCustomerRepo, InMemoryInventoryItemRepo,
    InMemoryLocationRepo, InMemoryMarketRepo, InMemoryPriceListRepo, InMemoryProductRepo,
    InMemoryStoreRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo,
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
    /// The one store this deployment serves (design note 0003, model A), set at startup.
    pub store: StoreId,
}

impl<S: Storage> AppState<S> {
    /// Ports scoped to the store this request is for. With one store per deployment
    /// that's always `self.store`; a multi-store host would resolve it from the request
    /// (API key, hostname) here, and nothing else would change.
    pub async fn ports(&self) -> Result<S::Ports<'_>, ApiError> {
        self.storage.ports(self.store).await
    }
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
    type Directory<'a>: StoreRepo
    where
        Self: 'a;

    /// Repositories that only see `store`'s data (design note 0003). With one store per
    /// deployment (model A) the database *is* the store, so implementations ignore it;
    /// shared tables (B) would filter by it, a database per store (C) would pick by it.
    async fn ports(&self, store: StoreId) -> Result<Self::Ports<'_>, ApiError>;

    /// The stores this deployment knows (in model C, a control plane).
    async fn directory(&self) -> Result<Self::Directory<'_>, ApiError>;
}

// ---------- in memory (default; data is lost on restart) ----------

#[derive(Default)]
pub struct MemoryStorage {
    stores: InMemoryStoreRepo,
    products: InMemoryProductRepo,
    markets: InMemoryMarketRepo,
    catalogs: InMemoryCatalogRepo,
    price_lists: InMemoryPriceListRepo,
    carts: InMemoryCartRepo,
    tax_classes: InMemoryTaxClassRepo,
    tax_regions: InMemoryTaxRegionRepo,
    inventory_items: InMemoryInventoryItemRepo,
    locations: InMemoryLocationRepo,
    customers: InMemoryCustomerRepo,
}

impl CartPorts for MemoryStorage {
    type Carts = InMemoryCartRepo;
    type Products = InMemoryProductRepo;
    type Markets = InMemoryMarketRepo;
    type Catalogs = InMemoryCatalogRepo;
    type PriceLists = InMemoryPriceListRepo;
    type TaxClasses = InMemoryTaxClassRepo;
    type TaxRegions = InMemoryTaxRegionRepo;
    type InventoryItems = InMemoryInventoryItemRepo;
    type Locations = InMemoryLocationRepo;
    type Customers = InMemoryCustomerRepo;

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
    fn inventory_items(&self) -> &Self::InventoryItems {
        &self.inventory_items
    }
    fn locations(&self) -> &Self::Locations {
        &self.locations
    }
    fn customers(&self) -> &Self::Customers {
        &self.customers
    }
}

impl Storage for MemoryStorage {
    type Ports<'a> = &'a MemoryStorage;
    type Directory<'a> = &'a InMemoryStoreRepo;

    async fn ports(&self, _store: StoreId) -> Result<&MemoryStorage, ApiError> {
        Ok(self)
    }

    async fn directory(&self) -> Result<&InMemoryStoreRepo, ApiError> {
        Ok(&self.stores)
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

impl PgStorage {
    async fn store(&self) -> Result<PgStore<PooledClient>, ApiError> {
        let conn = self.pool.get().await.map_err(ApiError::internal)?;
        Ok(PgStore::new(PooledClient(conn)))
    }
}

impl Storage for PgStorage {
    type Ports<'a> = PgStore<PooledClient>;
    type Directory<'a> = PgStore<PooledClient>;

    async fn ports(&self, _store: StoreId) -> Result<PgStore<PooledClient>, ApiError> {
        self.store().await
    }

    async fn directory(&self) -> Result<PgStore<PooledClient>, ApiError> {
        self.store().await
    }
}
