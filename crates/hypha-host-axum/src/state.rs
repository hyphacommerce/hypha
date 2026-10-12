use std::sync::Arc;

use hypha_cart::CartPorts;
use hypha_core::IdGenerator;
use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo,
    InMemoryProductRepo,
};
use uuid::Uuid;

/// Real IDs: UUIDv7 (time-ordered), generated from the system clock and RNG.
#[derive(Default)]
pub struct UuidV7Ids;

impl IdGenerator for UuidV7Ids {
    fn new_uuid(&self) -> Uuid {
        Uuid::now_v7()
    }
}

/// Every repo the host serves from. In-memory for now; a Postgres adapter would
/// replace these types without touching the handlers.
#[derive(Default)]
pub struct AppState {
    pub products: InMemoryProductRepo,
    pub markets: InMemoryMarketRepo,
    pub catalogs: InMemoryCatalogRepo,
    pub price_lists: InMemoryPriceListRepo,
    pub carts: InMemoryCartRepo,
    pub ids: UuidV7Ids,
}

pub type SharedState = Arc<AppState>;

impl CartPorts for AppState {
    type Carts = InMemoryCartRepo;
    type Products = InMemoryProductRepo;
    type Markets = InMemoryMarketRepo;
    type Catalogs = InMemoryCatalogRepo;
    type PriceLists = InMemoryPriceListRepo;

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
}
