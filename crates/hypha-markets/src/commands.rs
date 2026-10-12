use hypha_core::{AuthError, CatalogId, Ctx, Currency, MarketId, PriceListId, RepoError, Scope};
use thiserror::Error;

use crate::{Catalog, CatalogContext, CatalogRepo, MarketRepo, PriceListRepo};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("catalog {0} not found")]
    CatalogNotFound(CatalogId),
    #[error("price list {0} not found")]
    PriceListNotFound(PriceListId),
    #[error("market {0} not found")]
    MarketNotFound(MarketId),
    /// Until exchange rates exist, a catalog's price list must be in its market's currency.
    #[error("price list is in {found:?}, but the market sells in {expected:?}")]
    CurrencyMismatch { expected: Currency, found: Currency },
}

/// Attach a price list to a catalog, or detach it with `None`.
///
/// Rejects a price list in another currency than the catalog's market up front,
/// so the mistake surfaces here instead of at checkout. Price resolution still
/// checks too, because stored data can predate this rule.
pub async fn assign_price_list<M: MarketRepo, C: CatalogRepo, P: PriceListRepo>(
    ctx: &Ctx,
    markets: &M,
    catalogs: &C,
    price_lists: &P,
    catalog_id: CatalogId,
    price_list_id: Option<PriceListId>,
) -> Result<Catalog, CommandError> {
    ctx.require(Scope::PricingWrite)?;

    let mut catalog = catalogs
        .get(catalog_id)
        .await?
        .ok_or(CommandError::CatalogNotFound(catalog_id))?;

    if let Some(list_id) = price_list_id {
        let list = price_lists
            .get(list_id)
            .await?
            .ok_or(CommandError::PriceListNotFound(list_id))?;

        let expected = match catalog.context() {
            CatalogContext::Market(market_id) => markets
                .get(market_id)
                .await?
                .ok_or(CommandError::MarketNotFound(market_id))?
                .currency(),
        };

        if list.currency() != expected {
            return Err(CommandError::CurrencyMismatch {
                expected,
                found: list.currency(),
            });
        }
    }

    catalog.set_price_list(price_list_id);
    catalogs.save(&catalog).await?;
    Ok(catalog)
}
