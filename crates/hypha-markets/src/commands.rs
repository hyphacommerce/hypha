use std::collections::{HashMap, HashSet};

use hypha_core::{
    AuthError, BasisPoints, CatalogId, CountryCode, CountryError, Ctx, Currency, ErrorCode,
    IdGenerator, MarketId, Money, MoneyError, Price, PriceListId, ProductId, RepoError, Scope,
    VariantId,
};
use thiserror::Error;

use crate::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketError, MarketRepo, PriceList,
    PriceListError, PriceListRepo,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Country(#[from] CountryError),
    #[error(transparent)]
    Market(#[from] MarketError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error(transparent)]
    PriceList(#[from] PriceListError),
    #[error("catalog {0} not found")]
    CatalogNotFound(CatalogId),
    #[error("price list {0} not found")]
    PriceListNotFound(PriceListId),
    #[error("market {0} not found")]
    MarketNotFound(MarketId),
    /// Until exchange rates exist, a catalog's price list must be in its market's currency.
    #[error("price list is in {found}, but the market sells in {expected}")]
    CurrencyMismatch { expected: Currency, found: Currency },
    #[error("country {country} already belongs to market {market}")]
    CountryInOtherMarket {
        country: CountryCode,
        market: MarketId,
    },
    #[error("variant {0} has more than one fixed price")]
    DuplicateFixedPrice(VariantId),
    #[error("market {market} already has catalog {catalog}")]
    CatalogExists {
        market: MarketId,
        catalog: CatalogId,
    },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Country(e) => e.code(),
            CommandError::Market(e) => e.code(),
            CommandError::Money(e) => e.code(),
            CommandError::PriceList(e) => e.code(),
            CommandError::CatalogNotFound(_) => "catalog_not_found",
            CommandError::PriceListNotFound(_) => "price_list_not_found",
            CommandError::MarketNotFound(_) => "market_not_found",
            CommandError::CurrencyMismatch { .. } => "price_list_currency_mismatch",
            CommandError::CountryInOtherMarket { .. } => "country_in_other_market",
            CommandError::DuplicateFixedPrice(_) => "duplicate_fixed_price",
            CommandError::CatalogExists { .. } => "catalog_exists",
        }
    }
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

pub struct CreateMarket {
    pub name: String,
    /// ISO 3166-1 alpha-2 codes, e.g. "RS".
    pub countries: Vec<String>,
    pub currency: Currency,
}

/// Creates a market. A country can belong to only one market.
pub async fn create_market<M: MarketRepo, I: IdGenerator>(
    ctx: &Ctx,
    markets: &M,
    ids: &I,
    input: CreateMarket,
) -> Result<Market, CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    let countries = input
        .countries
        .iter()
        .map(|c| CountryCode::new(c))
        .collect::<Result<Vec<_>, _>>()?;
    let market = Market::new(
        MarketId::from_uuid(ids.new_uuid()),
        input.name,
        countries,
        input.currency,
    )?;

    for existing in markets.list_all().await? {
        if let Some(country) = market.countries().iter().find(|c| existing.covers(**c)) {
            return Err(CommandError::CountryInOtherMarket {
                country: *country,
                market: existing.id(),
            });
        }
    }

    markets.save(&market).await?;
    Ok(market)
}

pub struct FixedPrice {
    pub variant: VariantId,
    pub price: Money,
}

pub struct CreatePriceList {
    pub currency: Currency,
    /// Basis points: -1000 = 10% off, 500 = 5% markup.
    pub adjustment: i32,
    pub fixed_prices: Vec<FixedPrice>,
}

pub async fn create_price_list<P: PriceListRepo, I: IdGenerator>(
    ctx: &Ctx,
    price_lists: &P,
    ids: &I,
    input: CreatePriceList,
) -> Result<PriceList, CommandError> {
    ctx.require(Scope::PricingWrite)?;

    let adjustment = BasisPoints::new(input.adjustment)?;
    let mut fixed_prices = HashMap::with_capacity(input.fixed_prices.len());
    for fixed in input.fixed_prices {
        let price = Price::new(fixed.price)?;
        // Collecting into a map would silently keep the last duplicate.
        if fixed_prices.insert(fixed.variant, price).is_some() {
            return Err(CommandError::DuplicateFixedPrice(fixed.variant));
        }
    }

    let list = PriceList::new(
        PriceListId::from_uuid(ids.new_uuid()),
        input.currency,
        adjustment,
        fixed_prices,
    )?;
    price_lists.save(&list).await?;
    Ok(list)
}

pub struct CreateCatalog {
    pub market: MarketId,
    pub products: Vec<ProductId>,
}

/// Creates the catalog for a market, without a price list (see `assign_price_list`).
///
/// Product IDs are not checked against the products crate: markets is a leaf domain
/// and can't depend on it. Unknown products simply never resolve to a price.
pub async fn create_catalog<M: MarketRepo, C: CatalogRepo, I: IdGenerator>(
    ctx: &Ctx,
    markets: &M,
    catalogs: &C,
    ids: &I,
    input: CreateCatalog,
) -> Result<Catalog, CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    if markets.get(input.market).await?.is_none() {
        return Err(CommandError::MarketNotFound(input.market));
    }

    let context = CatalogContext::Market(input.market);
    if let Some(existing) = catalogs.get_by_context(context).await? {
        return Err(CommandError::CatalogExists {
            market: input.market,
            catalog: existing.id(),
        });
    }

    let catalog = Catalog::new(
        CatalogId::from_uuid(ids.new_uuid()),
        context,
        input.products.into_iter().collect::<HashSet<_>>(),
        None,
    );
    catalogs.save(&catalog).await?;
    Ok(catalog)
}
