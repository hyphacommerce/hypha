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

pub struct CreatePriceList {
    pub currency: Currency,
    /// Basis points: -1000 = 10% off, 500 = 5% markup.
    pub adjustment: i32,
}

/// Creates an empty price list. Fixed prices are added with `set_fixed_prices`.
pub async fn create_price_list<P: PriceListRepo, I: IdGenerator>(
    ctx: &Ctx,
    price_lists: &P,
    ids: &I,
    input: CreatePriceList,
) -> Result<PriceList, CommandError> {
    ctx.require(Scope::PricingWrite)?;

    let adjustment = BasisPoints::new(input.adjustment)?;
    let list = PriceList::new(
        PriceListId::from_uuid(ids.new_uuid()),
        input.currency,
        adjustment,
    );
    price_lists.save(&list).await?;
    Ok(list)
}

pub struct FixedPrice {
    pub variant: VariantId,
    pub price: Money,
}

/// Inserts or replaces fixed prices on a price list.
///
/// All prices are validated before anything is stored: non-negative, in the list's
/// currency, each variant once. Variant IDs are not checked against the products crate
/// (markets is a leaf domain); a price for an unknown variant is simply never used.
pub async fn set_fixed_prices<P: PriceListRepo>(
    ctx: &Ctx,
    price_lists: &P,
    list_id: PriceListId,
    prices: Vec<FixedPrice>,
) -> Result<(), CommandError> {
    ctx.require(Scope::PricingWrite)?;

    let list = price_lists
        .get(list_id)
        .await?
        .ok_or(CommandError::PriceListNotFound(list_id))?;
    let prices = prices
        .into_iter()
        .map(|p| Ok((p.variant, Price::new(p.price)?)))
        .collect::<Result<Vec<_>, MoneyError>>()?;
    list.check_fixed_prices(&prices)?;

    price_lists.set_fixed_prices(list_id, &prices).await?;
    Ok(())
}

/// Removes fixed prices from a price list; those variants fall back to the adjustment.
pub async fn remove_fixed_prices<P: PriceListRepo>(
    ctx: &Ctx,
    price_lists: &P,
    list_id: PriceListId,
    variants: Vec<VariantId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::PricingWrite)?;

    if price_lists.get(list_id).await?.is_none() {
        return Err(CommandError::PriceListNotFound(list_id));
    }
    price_lists.remove_fixed_prices(list_id, &variants).await?;
    Ok(())
}

pub struct CreateCatalog {
    pub market: MarketId,
}

/// Creates an empty catalog for a market, without a price list.
/// Add products with `add_catalog_products` and a price list with `assign_price_list`.
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

    let catalog = Catalog::new(CatalogId::from_uuid(ids.new_uuid()), context, None);
    catalogs.save(&catalog).await?;
    Ok(catalog)
}

/// Makes products sellable in the catalog's context. Products already included are ignored.
///
/// Product IDs are not checked against the products crate: markets is a leaf domain
/// and can't depend on it. Unknown products simply never resolve to a price.
pub async fn add_catalog_products<C: CatalogRepo>(
    ctx: &Ctx,
    catalogs: &C,
    catalog_id: CatalogId,
    products: Vec<ProductId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    if catalogs.get(catalog_id).await?.is_none() {
        return Err(CommandError::CatalogNotFound(catalog_id));
    }
    catalogs.add_products(catalog_id, &products).await?;
    Ok(())
}

/// Stops selling products in the catalog's context. Products not included are ignored.
pub async fn remove_catalog_products<C: CatalogRepo>(
    ctx: &Ctx,
    catalogs: &C,
    catalog_id: CatalogId,
    products: Vec<ProductId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    if catalogs.get(catalog_id).await?.is_none() {
        return Err(CommandError::CatalogNotFound(catalog_id));
    }
    catalogs.remove_products(catalog_id, &products).await?;
    Ok(())
}
