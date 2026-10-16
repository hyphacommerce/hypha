use hypha_core::{
    AuthError, BasisPoints, CatalogId, CountryCode, CountryError, Ctx, Currency, ErrorCode,
    HasBatch, IdGenerator, MarketId, Money, MoneyError, Page, PageRequest, PendingEvent, Price,
    PriceListId, ProductId, RepoError, Scope, StageEvents, Subject, VariantId, report_duplicate,
};
use thiserror::Error;

use crate::{
    Catalog, CatalogContext, CatalogRepo, HasCatalogs, HasMarkets, HasPriceLists, Market,
    MarketError, MarketRepo, PriceList, PriceListError, PriceListRepo, StageCatalogProducts,
    StageFixedPrices,
    events::{
        CATALOG, CatalogProductsAdded, CatalogProductsRemoved, PRICE_LIST, PricesRemoved, PricesSet,
    },
    repo::keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
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
    catalogs.update(&mut catalog).await?;
    Ok(catalog)
}

pub struct CreateMarket {
    pub name: String,
    /// ISO 3166-1 alpha-2 codes, e.g. "RS".
    pub countries: Vec<String>,
    pub currency: Currency,
    pub prices_include_tax: bool,
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
    let mut market = Market::new(
        MarketId::from_uuid(ids.new_uuid()),
        input.name,
        countries,
        input.currency,
    )?;
    market.set_prices_include_tax(input.prices_include_tax);

    // Precise error in the normal case; the repo's unique key catches a concurrent insert.
    if let Some(conflict) = country_conflict(markets, &market).await? {
        return Err(conflict);
    }

    let written = markets.insert(&mut market).await;
    report_duplicate(written, keys::COUNTRY, country_conflict(markets, &market)).await?;
    Ok(market)
}

/// Changes the fields that are `Some`; the rest keep their values.
#[derive(Debug, Default)]
pub struct UpdateMarket {
    pub name: Option<String>,
    pub prices_include_tax: Option<bool>,
}

pub async fn update_market<M: MarketRepo>(
    ctx: &Ctx,
    markets: &M,
    id: MarketId,
    input: UpdateMarket,
) -> Result<Market, CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    let mut market = load_market(markets, id).await?;
    if let Some(name) = &input.name {
        market.rename(name)?;
    }
    if let Some(include) = input.prices_include_tax {
        market.set_prices_include_tax(include);
    }
    markets.update(&mut market).await?;
    Ok(market)
}

/// Starts selling to `country` (ISO 3166-1 alpha-2) in this market. A country belongs to
/// one market, so one that another market sells to is `CountryInOtherMarket`.
pub async fn add_market_country<M: MarketRepo>(
    ctx: &Ctx,
    markets: &M,
    id: MarketId,
    country: &str,
) -> Result<Market, CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    let country = CountryCode::new(country)?;
    let mut market = load_market(markets, id).await?;
    market.add_country(country)?;
    if let Some(conflict) = country_conflict(markets, &market).await? {
        return Err(conflict);
    }

    let written = markets.update(&mut market).await;
    report_duplicate(written, keys::COUNTRY, country_conflict(markets, &market)).await?;
    Ok(market)
}

/// Stops selling to `country` in this market. Its last country can't be removed.
/// Existing carts keep the market they were created in.
pub async fn remove_market_country<M: MarketRepo>(
    ctx: &Ctx,
    markets: &M,
    id: MarketId,
    country: &str,
) -> Result<Market, CommandError> {
    ctx.require(Scope::MarketsWrite)?;

    let country = CountryCode::new(country)?;
    let mut market = load_market(markets, id).await?;
    market.remove_country(country)?;
    markets.update(&mut market).await?;
    Ok(market)
}

async fn load_market<M: MarketRepo>(markets: &M, id: MarketId) -> Result<Market, CommandError> {
    markets
        .get(id)
        .await?
        .ok_or(CommandError::MarketNotFound(id))
}

/// `CountryInOtherMarket` for the first of `market`'s countries another market owns.
async fn country_conflict<M: MarketRepo>(
    markets: &M,
    market: &Market,
) -> Result<Option<CommandError>, CommandError> {
    for existing in markets.list_all().await? {
        if existing.id() == market.id() {
            continue;
        }
        if let Some(country) = market.countries().iter().find(|c| existing.covers(**c)) {
            return Ok(Some(CommandError::CountryInOtherMarket {
                country: *country,
                market: existing.id(),
            }));
        }
    }
    Ok(None)
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
    let mut list = PriceList::new(
        PriceListId::from_uuid(ids.new_uuid()),
        input.currency,
        adjustment,
    );
    price_lists.insert(&mut list).await?;
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
pub async fn set_fixed_prices<S>(
    ctx: &Ctx,
    ports: &S,
    list_id: PriceListId,
    prices: Vec<FixedPrice>,
) -> Result<(), CommandError>
where
    S: HasPriceLists + HasBatch<Batch: StageFixedPrices + StageEvents>,
{
    ctx.require(Scope::PricingWrite)?;

    let list = ports
        .price_lists()
        .get(list_id)
        .await?
        .ok_or(CommandError::PriceListNotFound(list_id))?;
    let prices = prices
        .into_iter()
        .map(|p| Ok((p.variant, Price::new(p.price)?)))
        .collect::<Result<Vec<_>, MoneyError>>()?;
    list.check_fixed_prices(&prices)?;

    let event = PricesSet {
        price_list: list_id,
        variants: prices.iter().map(|(v, _)| *v).collect(),
    };
    let mut batch = ports.batch();
    batch.set_fixed_prices(list_id, &prices);
    batch.record_events(vec![PendingEvent::new(price_list_subject(list_id), &event)]);
    Ok(ports.commit(batch).await?)
}

fn price_list_subject(list: PriceListId) -> Subject {
    Subject::new(PRICE_LIST, list.as_uuid())
}

fn catalog_subject(catalog: CatalogId) -> Subject {
    Subject::new(CATALOG, catalog.as_uuid())
}

/// Removes fixed prices from a price list; those variants fall back to the adjustment.
pub async fn remove_fixed_prices<S>(
    ctx: &Ctx,
    ports: &S,
    list_id: PriceListId,
    variants: Vec<VariantId>,
) -> Result<(), CommandError>
where
    S: HasPriceLists + HasBatch<Batch: StageFixedPrices + StageEvents>,
{
    ctx.require(Scope::PricingWrite)?;

    if ports.price_lists().get(list_id).await?.is_none() {
        return Err(CommandError::PriceListNotFound(list_id));
    }
    let mut batch = ports.batch();
    batch.remove_fixed_prices(list_id, &variants);
    let event = PricesRemoved {
        price_list: list_id,
        variants,
    };
    batch.record_events(vec![PendingEvent::new(price_list_subject(list_id), &event)]);
    Ok(ports.commit(batch).await?)
}

/// A price list's changes; `None` keeps a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePriceList {
    /// Basis points (-1000 = 10% off).
    pub adjustment: Option<i32>,
}

/// Changes a price list's adjustment. Its currency is fixed: catalogs and fixed prices
/// depend on it.
pub async fn update_price_list<P: PriceListRepo>(
    ctx: &Ctx,
    lists: &P,
    id: PriceListId,
    input: UpdatePriceList,
) -> Result<PriceList, CommandError> {
    ctx.require(Scope::PricingWrite)?;
    let mut list = lists
        .get(id)
        .await?
        .ok_or(CommandError::PriceListNotFound(id))?;
    if let Some(adjustment) = input.adjustment {
        list.set_adjustment(BasisPoints::new(adjustment)?);
    }
    lists.update(&mut list).await?;
    Ok(list)
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
    // Precise error in the normal case; the repo's unique key catches a concurrent insert.
    if let Some(exists) = catalog_exists(catalogs, input.market).await? {
        return Err(exists);
    }
    let mut catalog = Catalog::new(CatalogId::from_uuid(ids.new_uuid()), context, None);
    let written = catalogs.insert(&mut catalog).await;
    report_duplicate(
        written,
        keys::CATALOG_CONTEXT,
        catalog_exists(catalogs, input.market),
    )
    .await?;
    Ok(catalog)
}

/// `CatalogExists` naming `market`'s catalog, if it has one.
async fn catalog_exists<C: CatalogRepo>(
    catalogs: &C,
    market: MarketId,
) -> Result<Option<CommandError>, CommandError> {
    Ok(catalogs
        .get_by_context(CatalogContext::Market(market))
        .await?
        .map(|existing| CommandError::CatalogExists {
            market,
            catalog: existing.id(),
        }))
}

/// Makes products sellable in the catalog's context. Products already included are ignored.
///
/// Product IDs are not checked against the products crate: markets is a leaf domain
/// and can't depend on it. Unknown products simply never resolve to a price.
pub async fn add_catalog_products<S>(
    ctx: &Ctx,
    ports: &S,
    catalog_id: CatalogId,
    products: Vec<ProductId>,
) -> Result<(), CommandError>
where
    S: HasCatalogs + HasBatch<Batch: StageCatalogProducts + StageEvents>,
{
    ctx.require(Scope::MarketsWrite)?;

    if ports.catalogs().get(catalog_id).await?.is_none() {
        return Err(CommandError::CatalogNotFound(catalog_id));
    }
    let mut batch = ports.batch();
    batch.add_catalog_products(catalog_id, &products);
    let event = CatalogProductsAdded {
        catalog: catalog_id,
        products,
    };
    batch.record_events(vec![PendingEvent::new(catalog_subject(catalog_id), &event)]);
    Ok(ports.commit(batch).await?)
}

/// Stops selling products in the catalog's context. Products not included are ignored.
pub async fn remove_catalog_products<S>(
    ctx: &Ctx,
    ports: &S,
    catalog_id: CatalogId,
    products: Vec<ProductId>,
) -> Result<(), CommandError>
where
    S: HasCatalogs + HasBatch<Batch: StageCatalogProducts + StageEvents>,
{
    ctx.require(Scope::MarketsWrite)?;

    if ports.catalogs().get(catalog_id).await?.is_none() {
        return Err(CommandError::CatalogNotFound(catalog_id));
    }
    let mut batch = ports.batch();
    batch.remove_catalog_products(catalog_id, &products);
    let event = CatalogProductsRemoved {
        catalog: catalog_id,
        products,
    };
    batch.record_events(vec![PendingEvent::new(catalog_subject(catalog_id), &event)]);
    Ok(ports.commit(batch).await?)
}

// ---------- reads (design note 0021, SU3) ----------

/// A market as the admin reads it: the market and the ID of its catalog, if it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketView {
    pub market: Market,
    pub catalog: Option<CatalogId>,
}

/// Every market, oldest first, each with its catalog.
pub async fn list_markets<S: HasMarkets + HasCatalogs>(
    ctx: &Ctx,
    ports: &S,
) -> Result<Vec<MarketView>, CommandError> {
    ctx.require(Scope::MarketsRead)?;
    let markets = ports.markets().list_all().await?;
    let catalogs = ports.catalogs().list_all().await?;
    Ok(markets
        .into_iter()
        .map(|market| {
            let catalog = catalogs
                .iter()
                .find(|c| c.context() == CatalogContext::Market(market.id()))
                .map(|c| c.id());
            MarketView { market, catalog }
        })
        .collect())
}

pub async fn get_market<S: HasMarkets + HasCatalogs>(
    ctx: &Ctx,
    ports: &S,
    id: MarketId,
) -> Result<MarketView, CommandError> {
    ctx.require(Scope::MarketsRead)?;
    let market = ports
        .markets()
        .get(id)
        .await?
        .ok_or(CommandError::MarketNotFound(id))?;
    let catalog = ports
        .catalogs()
        .get_by_context(CatalogContext::Market(id))
        .await?
        .map(|c| c.id());
    Ok(MarketView { market, catalog })
}

pub async fn get_catalog<C: CatalogRepo>(
    ctx: &Ctx,
    catalogs: &C,
    id: CatalogId,
) -> Result<Catalog, CommandError> {
    ctx.require(Scope::MarketsRead)?;
    catalogs
        .get(id)
        .await?
        .ok_or(CommandError::CatalogNotFound(id))
}

/// A page of the catalog's products, newest first. Only IDs: markets is a leaf and
/// doesn't know products; callers load them.
pub async fn list_catalog_products<C: CatalogRepo>(
    ctx: &Ctx,
    catalogs: &C,
    id: CatalogId,
    page: PageRequest<ProductId>,
) -> Result<Page<ProductId>, CommandError> {
    ctx.require(Scope::MarketsRead)?;
    if catalogs.get(id).await?.is_none() {
        return Err(CommandError::CatalogNotFound(id));
    }
    Ok(catalogs.product_page(id, page).await?)
}

/// Every price list, oldest first.
pub async fn list_price_lists<P: PriceListRepo>(
    ctx: &Ctx,
    lists: &P,
) -> Result<Vec<PriceList>, CommandError> {
    ctx.require(Scope::PricingRead)?;
    Ok(lists.list_all().await?)
}

pub async fn get_price_list<P: PriceListRepo>(
    ctx: &Ctx,
    lists: &P,
    id: PriceListId,
) -> Result<PriceList, CommandError> {
    ctx.require(Scope::PricingRead)?;
    lists
        .get(id)
        .await?
        .ok_or(CommandError::PriceListNotFound(id))
}

/// A page of the list's fixed prices, newest variant first.
pub async fn list_fixed_prices<P: PriceListRepo>(
    ctx: &Ctx,
    lists: &P,
    id: PriceListId,
    page: PageRequest<VariantId>,
) -> Result<Page<(VariantId, Price)>, CommandError> {
    ctx.require(Scope::PricingRead)?;
    if lists.get(id).await?.is_none() {
        return Err(CommandError::PriceListNotFound(id));
    }
    Ok(lists.fixed_price_page(id, page).await?)
}
