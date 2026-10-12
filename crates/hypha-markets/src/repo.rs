use hypha_core::{CatalogId, MarketId, PriceListId, RepoError};

use crate::{
    catalog::{Catalog, CatalogContext},
    market::Market,
    price_list::PriceList,
};

#[allow(async_fn_in_trait)]
pub trait MarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError>;
    async fn save(&self, market: &Market) -> Result<(), RepoError>;
    async fn list_all(&self) -> Result<Vec<Market>, RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait CatalogRepo {
    async fn get(&self, id: CatalogId) -> Result<Option<Catalog>, RepoError>;
    /// At most one catalog per context, see `check_one_catalog_per_context`.
    async fn get_by_context(&self, context: CatalogContext) -> Result<Option<Catalog>, RepoError>;
    async fn save(&self, catalog: &Catalog) -> Result<(), RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait PriceListRepo {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError>;
    async fn save(&self, price_list: &PriceList) -> Result<(), RepoError>;
}
