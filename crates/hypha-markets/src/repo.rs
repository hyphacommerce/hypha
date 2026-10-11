use hypha_core::{MarketId, RepoError};

use crate::market::Market;

#[allow(async_fn_in_trait)]
pub trait MarketRepo {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError>;
    async fn save(&self, market: &Market) -> Result<(), RepoError>;
    async fn list_all(&self) -> Result<Vec<Market>, RepoError>;
}
