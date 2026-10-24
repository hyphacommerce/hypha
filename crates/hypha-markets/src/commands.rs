use hypha_core::{
    AuthError, BasisPoints, CatalogId, CountryCode, CountryError, Ctx, Currency, CustomerGroupId,
    ErrorCode, HasBatch, IdGenerator, Keyset, MarketId, Money, MoneyError, Page, PageRequest,
    PendingEvent, Price, PriceListId, ProductId, RepoError, Scope, StageEvents, Subject, Timestamp,
    VariantId, report_duplicate,
};
use thiserror::Error;

use crate::{
    Catalog, CatalogContext, CatalogRepo, HasCatalogs, HasMarkets, HasPriceLists, Market,
    MarketError, MarketRepo, PriceList, PriceListError, PriceListRepo, StageCatalogProducts,
    StageFixedPrices,
    error::FixedPriceProblem,
    events::{
        CATALOG, CatalogProductsAdded, CatalogProductsRemoved, PRICE_LIST, PricesRemoved, PricesSet,
    },
    filter::{FixedPriceFilter, PriceListFilter, PriceListSort},
    price_list::{
        Adjustment, AppliesTo, Audience, CompareAtMode, EntryProblem, FixedPrice, PriceBreak,
        PriceListDraft, PriceListKind, PriceListPhase, PriceListStatus, Window,
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
    #[error("price list {price_list} is already named {name:?}")]
    PriceListNameTaken {
        name: String,
        price_list: PriceListId,
    },
    #[error("at most {max} fixed prices can be set at once")]
    TooManyPrices { max: usize },
    /// The catalogs holding it, by ID.
    #[error("price list {price_list} is on a catalog")]
    PriceListInUse {
        price_list: PriceListId,
        catalogs: Vec<CatalogId>,
    },
    /// The audience names a group that doesn't exist (storage's check: this leaf can't
    /// see groups).
    #[error("a customer group the audience names doesn't exist")]
    AudienceGroupNotFound,
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
            CommandError::PriceListNameTaken { .. } => "price_list_name_taken",
            CommandError::TooManyPrices { .. } => "too_many_prices",
            CommandError::PriceListInUse { .. } => "price_list_in_use",
            CommandError::AudienceGroupNotFound => "customer_group_not_found",
        }
    }
}

/// Replaces a catalog's price lists with `lists`, which compete to price its products (the
/// lowest live price wins).
///
/// Every list must exist and be in the market's currency: the mistake surfaces here
/// instead of at checkout. Price resolution still checks too, because stored data can
/// predate this rule.
pub async fn set_catalog_price_lists<S: HasMarkets + HasCatalogs + HasPriceLists>(
    ctx: &Ctx,
    ports: &S,
    catalog_id: CatalogId,
    lists: Vec<PriceListId>,
) -> Result<Catalog, CommandError> {
    ctx.require(Scope::PricingWrite)?;
    let mut catalog = load_catalog(ports.catalogs(), catalog_id).await?;
    catalog.set_price_lists(lists)?;
    save_catalog_lists(ports, &mut catalog).await?;
    Ok(catalog)
}

/// Adds one list to a catalog's; one already there changes nothing.
pub async fn add_catalog_price_list<S: HasMarkets + HasCatalogs + HasPriceLists>(
    ctx: &Ctx,
    ports: &S,
    catalog_id: CatalogId,
    list: PriceListId,
) -> Result<Catalog, CommandError> {
    ctx.require(Scope::PricingWrite)?;
    let mut catalog = load_catalog(ports.catalogs(), catalog_id).await?;
    catalog.add_price_list(list)?;
    save_catalog_lists(ports, &mut catalog).await?;
    Ok(catalog)
}

/// Takes one list off a catalog (`PriceListNotInCatalog` when it isn't on it).
pub async fn remove_catalog_price_list<S: HasCatalogs>(
    ctx: &Ctx,
    ports: &S,
    catalog_id: CatalogId,
    list: PriceListId,
) -> Result<Catalog, CommandError> {
    ctx.require(Scope::PricingWrite)?;
    let mut catalog = load_catalog(ports.catalogs(), catalog_id).await?;
    catalog.remove_price_list(list)?;
    ports.catalogs().update(&mut catalog).await?;
    Ok(catalog)
}

async fn load_catalog<C: CatalogRepo>(
    catalogs: &C,
    id: CatalogId,
) -> Result<Catalog, CommandError> {
    catalogs
        .get(id)
        .await?
        .ok_or(CommandError::CatalogNotFound(id))
}

/// Checks the catalog's lists exist and are in its market's currency, then stores it. A
/// list deleted after the check is the storage rule's (`Reference`), read as missing.
async fn save_catalog_lists<S: HasMarkets + HasCatalogs + HasPriceLists>(
    ports: &S,
    catalog: &mut Catalog,
) -> Result<(), CommandError> {
    let found = ports.price_lists().get_many(catalog.price_lists()).await?;
    if let Some(missing) = catalog
        .price_lists()
        .iter()
        .find(|id| !found.iter().any(|l| l.id() == **id))
    {
        return Err(CommandError::PriceListNotFound(*missing));
    }
    let expected = match catalog.context() {
        CatalogContext::Market(market_id) => ports
            .markets()
            .get(market_id)
            .await?
            .ok_or(CommandError::MarketNotFound(market_id))?
            .currency(),
    };
    if let Some(other) = found.iter().find(|l| l.currency() != expected) {
        return Err(CommandError::CurrencyMismatch {
            expected,
            found: other.currency(),
        });
    }
    match ports.catalogs().update(catalog).await {
        Err(RepoError::Reference(keys::CATALOG_PRICE_LIST)) => {
            let found = ports.price_lists().get_many(catalog.price_lists()).await?;
            let missing = catalog
                .price_lists()
                .iter()
                .copied()
                .find(|id| !found.iter().any(|l| l.id() == *id));
            Err(match missing {
                Some(id) => CommandError::PriceListNotFound(id),
                None => RepoError::Reference(keys::CATALOG_PRICE_LIST).into(),
            })
        }
        done => Ok(done?),
    }
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
    pub name: String,
    pub description: Option<String>,
    pub currency: Currency,
    pub kind: PriceListKind,
    pub status: PriceListStatus,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub compare_at: CompareAtMode,
    /// `None` adjusts nothing, so the list prices only the variants it has fixed prices
    /// for.
    pub adjustment: Option<AdjustmentInput>,
    /// The customer groups it prices for; none is everyone.
    pub groups: Vec<CustomerGroupId>,
}

/// An adjustment as a caller sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustmentInput {
    /// Basis points: -1000 = 10% off, 500 = 5% markup.
    pub percent: i32,
    pub applies_to: AppliesTo,
}

fn adjustment(input: AdjustmentInput) -> Result<Adjustment, CommandError> {
    Ok(Adjustment::new(
        BasisPoints::new(input.percent)?,
        input.applies_to,
    )?)
}

/// Creates a price list without fixed prices (add them with `set_fixed_prices`). Its
/// name is unique, ignoring case.
pub async fn create_price_list<P: PriceListRepo, I: IdGenerator>(
    ctx: &Ctx,
    price_lists: &P,
    ids: &I,
    input: CreatePriceList,
) -> Result<PriceList, CommandError> {
    ctx.require(Scope::PricingWrite)?;

    let adjustment = input.adjustment.map(adjustment).transpose()?;
    let audience = Audience::of_groups(input.groups)?;
    let mut list = PriceList::new(
        PriceListId::from_uuid(ids.new_uuid()),
        PriceListDraft {
            name: input.name,
            description: input.description,
            currency: input.currency,
            kind: input.kind,
            status: input.status,
            starts_at: input.starts_at,
            ends_at: input.ends_at,
            compare_at: input.compare_at,
            adjustment,
            audience,
        },
    )?;
    // Precise error in the normal case; the repo's unique key catches a concurrent insert.
    if let Some(taken) = name_taken(price_lists, &list).await? {
        return Err(taken);
    }
    let written = unknown_group(price_lists.insert(&mut list).await)?;
    report_duplicate(
        written,
        keys::PRICE_LIST_NAME,
        name_taken(price_lists, &list),
    )
    .await?;
    Ok(list)
}

/// A write refused for an audience group that doesn't exist as `AudienceGroupNotFound`;
/// everything else (a duplicate name included) passed on for the caller to judge.
fn unknown_group(written: Result<(), RepoError>) -> Result<Result<(), RepoError>, CommandError> {
    match written {
        Err(RepoError::Reference(key)) if key == keys::PRICE_LIST_GROUP => {
            Err(CommandError::AudienceGroupNotFound)
        }
        other => Ok(other),
    }
}

/// `PriceListNameTaken` when another list has `list`'s name, ignoring case.
async fn name_taken<P: PriceListRepo>(
    price_lists: &P,
    list: &PriceList,
) -> Result<Option<CommandError>, CommandError> {
    let name = list.name().to_lowercase();
    Ok(price_lists
        .list_all()
        .await?
        .into_iter()
        .find(|other| other.id() != list.id() && other.name().to_lowercase() == name)
        .map(|other| CommandError::PriceListNameTaken {
            name: other.name().to_string(),
            price_list: other.id(),
        }))
}

/// How many fixed prices one `set_fixed_prices` takes: bulk callers page through.
pub const MAX_FIXED_PRICES_PER_WRITE: usize = 1000;

/// One fixed price as a caller sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetFixedPrice {
    pub variant: VariantId,
    pub price: Money,
    /// Shown struck through beside the price; above it, in its currency.
    pub compare_at: Option<Money>,
    /// Quantity prices: `(min_quantity, price)`, by quantity.
    pub breaks: Vec<(u32, Money)>,
}

/// Inserts or replaces fixed prices on a price list.
///
/// All prices are validated before anything is stored: non-negative, in the list's
/// currency, a compare-at above its price, each variant once. Every entry's problem is
/// reported at once (`InvalidFixedPrices`). Variant IDs are not checked against the
/// products crate (markets is a leaf domain); a price for an unknown variant is simply
/// never used.
pub async fn set_fixed_prices<S>(
    ctx: &Ctx,
    ports: &S,
    list_id: PriceListId,
    prices: Vec<SetFixedPrice>,
) -> Result<(), CommandError>
where
    S: HasPriceLists + HasBatch<Batch: StageFixedPrices + StageEvents>,
{
    ctx.require(Scope::PricingWrite)?;
    if prices.len() > MAX_FIXED_PRICES_PER_WRITE {
        return Err(CommandError::TooManyPrices {
            max: MAX_FIXED_PRICES_PER_WRITE,
        });
    }

    let list = ports
        .price_lists()
        .get(list_id)
        .await?
        .ok_or(CommandError::PriceListNotFound(list_id))?;
    let mut problems = Vec::new();
    let mut parsed = Vec::with_capacity(prices.len());
    // Each parsed entry's index in the request, so the list's own checks report it.
    let mut indexes = Vec::with_capacity(prices.len());
    for (index, entry) in prices.into_iter().enumerate() {
        match fixed_price(&entry) {
            Ok(fixed) => {
                indexes.push(index);
                parsed.push((entry.variant, fixed));
            }
            Err(problem) => problems.push(EntryProblem {
                index,
                variant: Some(entry.variant),
                problem,
            }),
        }
    }
    if let Err(PriceListError::InvalidFixedPrices(found)) = list.check_fixed_prices(&parsed) {
        problems.extend(found.into_iter().map(|p| EntryProblem {
            index: indexes[p.index],
            ..p
        }));
    }
    if !problems.is_empty() {
        problems.sort_by_key(|p| p.index);
        return Err(PriceListError::InvalidFixedPrices(problems).into());
    }

    let event = PricesSet {
        price_list: list_id,
        variants: parsed.iter().map(|(v, _)| *v).collect(),
    };
    let mut batch = ports.batch();
    batch.set_fixed_prices(list_id, &parsed);
    batch.record_events(vec![PendingEvent::new(price_list_subject(list_id), &event)]);
    Ok(ports.commit(batch).await?)
}

/// One entry's own rules: amounts that are prices, a compare-at above the price.
fn fixed_price(entry: &SetFixedPrice) -> Result<FixedPrice, FixedPriceProblem> {
    let price = Price::new(entry.price)?;
    let compare_at = entry.compare_at.map(Price::new).transpose()?;
    let breaks = entry
        .breaks
        .iter()
        .map(|(min_quantity, price)| {
            Ok(PriceBreak {
                min_quantity: *min_quantity,
                price: Price::new(*price)?,
            })
        })
        .collect::<Result<Vec<_>, FixedPriceProblem>>()?;
    FixedPrice::new(price, compare_at)?.with_breaks(breaks)
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

/// A price list's changes; `None` keeps a field, and `Some(None)` clears one that may be
/// empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePriceList {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub kind: Option<PriceListKind>,
    pub status: Option<PriceListStatus>,
    pub starts_at: Option<Option<Timestamp>>,
    pub ends_at: Option<Option<Timestamp>>,
    pub compare_at: Option<CompareAtMode>,
    pub adjustment: Option<Option<AdjustmentInput>>,
    /// The audience's groups, replacing them; none is everyone.
    pub groups: Option<Vec<CustomerGroupId>>,
}

/// Changes a price list. Its currency is fixed: catalogs and fixed prices depend on it.
/// The window is checked as it ends up, so moving both ends at once works in either
/// order.
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
    if let Some(name) = &input.name {
        list.rename(name)?;
    }
    if let Some(description) = input.description {
        list.set_description(description)?;
    }
    if let Some(kind) = input.kind {
        list.set_kind(kind);
    }
    if let Some(status) = input.status {
        list.set_status(status);
    }
    if input.starts_at.is_some() || input.ends_at.is_some() {
        let window = list.window();
        let starts_at = input.starts_at.unwrap_or(window.starts_at());
        let ends_at = input.ends_at.unwrap_or(window.ends_at());
        list.set_window(Window::new(starts_at, ends_at)?);
    }
    if let Some(mode) = input.compare_at {
        list.set_compare_at(mode);
    }
    if let Some(new) = input.adjustment {
        list.set_adjustment(new.map(adjustment).transpose()?);
    }
    if let Some(groups) = input.groups {
        list.set_audience(Audience::of_groups(groups)?);
    }
    if input.name.is_some()
        && let Some(taken) = name_taken(lists, &list).await?
    {
        return Err(taken);
    }
    let written = unknown_group(lists.update(&mut list).await)?;
    report_duplicate(written, keys::PRICE_LIST_NAME, name_taken(lists, &list)).await?;
    Ok(list)
}

/// Deletes a price list with its fixed prices. Refused while a catalog holds it
/// (`PriceListInUse`): take it off them first.
pub async fn delete_price_list<S: HasCatalogs + HasPriceLists>(
    ctx: &Ctx,
    ports: &S,
    id: PriceListId,
) -> Result<(), CommandError> {
    ctx.require(Scope::PricingWrite)?;
    let mut list = ports
        .price_lists()
        .get(id)
        .await?
        .ok_or(CommandError::PriceListNotFound(id))?;
    if let Some(in_use) = in_use(ports, id).await? {
        return Err(in_use);
    }
    list.delete();
    match ports.price_lists().delete(&mut list).await {
        // A catalog took it on after the check.
        Err(RepoError::Reference(keys::CATALOG_PRICE_LIST)) => Err(in_use(ports, id)
            .await?
            .unwrap_or(RepoError::Reference(keys::CATALOG_PRICE_LIST).into())),
        done => Ok(done?),
    }
}

/// `PriceListInUse` naming the catalogs that hold `list`, if any do.
async fn in_use<S: HasCatalogs>(
    ports: &S,
    list: PriceListId,
) -> Result<Option<CommandError>, CommandError> {
    let catalogs: Vec<CatalogId> = ports
        .catalogs()
        .list_all()
        .await?
        .iter()
        .filter(|c| c.price_lists().contains(&list))
        .map(|c| c.id())
        .collect();
    Ok(
        (!catalogs.is_empty()).then_some(CommandError::PriceListInUse {
            price_list: list,
            catalogs,
        }),
    )
}

pub struct CreateCatalog {
    pub market: MarketId,
}

/// Creates an empty catalog for a market, without price lists. Add products with
/// `add_catalog_products` and price lists with `set_catalog_price_lists`.
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
    let mut catalog = Catalog::new(CatalogId::from_uuid(ids.new_uuid()), context, []);
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

// ---------- reads ----------

/// A market as the admin reads it: the market, the ID of its catalog if it has one, and
/// the catalog's price lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketView {
    pub market: Market,
    pub catalog: Option<CatalogId>,
    pub price_lists: Vec<PriceListId>,
}

/// Every market, oldest first, without their catalogs: what a storefront's country list
/// needs, in one read.
pub async fn list_markets_only<M: MarketRepo>(
    ctx: &Ctx,
    markets: &M,
) -> Result<Vec<Market>, CommandError> {
    ctx.require(Scope::MarketsRead)?;
    Ok(markets.list_all().await?)
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
                .find(|c| c.context() == CatalogContext::Market(market.id()));
            MarketView {
                market,
                catalog: catalog.map(|c| c.id()),
                price_lists: catalog
                    .map(|c| c.price_lists().to_vec())
                    .unwrap_or_default(),
            }
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
        .await?;
    Ok(MarketView {
        market,
        catalog: catalog.as_ref().map(|c| c.id()),
        price_lists: catalog
            .map(|c| c.price_lists().to_vec())
            .unwrap_or_default(),
    })
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

/// What a list of price lists is narrowed to, as a request gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListPriceLists {
    pub kind: Option<PriceListKind>,
    /// The phase lists are in now (`at`).
    pub phase: Option<PriceListPhase>,
    /// Only the lists on this market's catalog; a market without one has none.
    pub market: Option<MarketId>,
    /// Only the lists whose audience names this group.
    pub group: Option<CustomerGroupId>,
    pub sort: PriceListSort,
}

/// A page of price lists in `input.sort`'s order. `at` is now: what phases are judged at.
pub async fn list_price_lists<S: HasMarkets + HasCatalogs + HasPriceLists>(
    ctx: &Ctx,
    ports: &S,
    input: ListPriceLists,
    at: Timestamp,
    page: PageRequest<Keyset<PriceListId>>,
) -> Result<Page<PriceList>, CommandError> {
    ctx.require(Scope::PricingRead)?;
    let ids = match input.market {
        Some(market) => {
            if ports.markets().get(market).await?.is_none() {
                return Err(CommandError::MarketNotFound(market));
            }
            Some(
                ports
                    .catalogs()
                    .get_by_context(CatalogContext::Market(market))
                    .await?
                    .map(|c| c.price_lists().to_vec())
                    .unwrap_or_default(),
            )
        }
        None => None,
    };
    let filter = PriceListFilter {
        kind: input.kind,
        phase: input.phase,
        at,
        ids,
        group: input.group,
    };
    Ok(ports.price_lists().list(&filter, input.sort, page).await?)
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

/// A page of the list's fixed prices, newest variant first, narrowed by `filter`.
pub async fn list_fixed_prices<P: PriceListRepo>(
    ctx: &Ctx,
    lists: &P,
    id: PriceListId,
    filter: &FixedPriceFilter,
    page: PageRequest<VariantId>,
) -> Result<Page<(VariantId, FixedPrice)>, CommandError> {
    ctx.require(Scope::PricingRead)?;
    if lists.get(id).await?.is_none() {
        return Err(CommandError::PriceListNotFound(id));
    }
    Ok(lists.fixed_price_page(id, filter, page).await?)
}
