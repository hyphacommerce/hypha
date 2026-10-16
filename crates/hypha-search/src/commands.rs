//! Search's commands (design note 0019, sections 6 and 7): searching a market, the
//! merchant's settings, a full rebuild and how far behind the index is.

use std::collections::BTreeMap;

use hypha_core::{
    AuthError, CountryCode, CountryError, Ctx, ErrorCode, RepoError, Scope, optional_text,
};
use hypha_markets::{HasMarkets, Market, MarketRepo};
use thiserror::Error;

use crate::{
    FacetKey, FacetSetting, Filter, Group, HasSearchQueue, HasSearchSettings, IndexError,
    MAX_DEPTH, MAX_LIMIT, PriceBounds, SearchError, SearchIndex, SearchQuery, SearchQueue,
    SearchResults, SearchSettings, SearchSettingsRepo, Sort, queue_all, terms,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Search(#[from] SearchError),
    #[error(transparent)]
    Country(#[from] CountryError),
    #[error("no market sells to {0}")]
    NoMarketForCountry(CountryCode),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Search(e) => e.code(),
            CommandError::Country(e) => e.code(),
            CommandError::NoMarketForCountry(_) => "no_market_for_country",
            CommandError::Repo(e) => e.code(),
            CommandError::Index(e) => e.code(),
        }
    }
}

/// A search as a request gives it; `search` checks it.
#[derive(Debug, Clone, Default)]
pub struct SearchInput {
    /// Where the shopper is (ISO 3166-1 alpha-2): its market prices the results.
    pub country: String,
    /// Words to find; blank lists everything.
    pub q: Option<String>,
    /// `key:value`, key a `FacetKey` (`option.color:Red`, `vendor:Acme`): values of one
    /// key OR, keys AND.
    pub filters: Vec<String>,
    /// Inclusive, in the market currency's minor units.
    pub price_min: Option<i64>,
    pub price_max: Option<i64>,
    pub available: Option<bool>,
    pub sort: Sort,
    pub group: Group,
    pub offset: u32,
    pub limit: u32,
}

/// What a search found, in the market it searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub market: Market,
    pub results: SearchResults,
    /// The labels of the facets in `results`, where the merchant set one.
    pub labels: BTreeMap<FacetKey, String>,
}

/// How many default option facets a search counts (the most used options).
pub const DEFAULT_OPTION_FACETS: usize = 5;

/// Searches the market serving `input.country` (design note 0019, SE6). The facets
/// counted are the merchant's (`SearchSettings::facets`), else product type, vendor,
/// the market's most used options and tags.
pub async fn search<S: HasMarkets + HasSearchSettings, I: SearchIndex>(
    ctx: &Ctx,
    ports: &S,
    index: &I,
    input: SearchInput,
) -> Result<Found, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let country = CountryCode::new(&input.country)?;
    let market = ports
        .markets()
        .get_by_country(country)
        .await?
        .ok_or(CommandError::NoMarketForCountry(country))?;

    let terms = optional_text("q", input.q)
        .map_err(SearchError::Text)?
        .map(|q| terms::terms(&q))
        .unwrap_or_default();
    let filters = parse_filters(&input.filters)?;
    if let (Some(min), Some(max)) = (input.price_min, input.price_max)
        && min > max
    {
        return Err(SearchError::InvalidPriceRange.into());
    }
    if input.limit == 0 || input.limit > MAX_LIMIT {
        return Err(SearchError::InvalidLimit { max: MAX_LIMIT }.into());
    }
    if input.offset.saturating_add(input.limit) > MAX_DEPTH {
        return Err(SearchError::TooDeep { max: MAX_DEPTH }.into());
    }

    let settings = ports.search_settings().get().await?.unwrap_or_default();
    let (facets, labels) = if settings.facets().is_empty() {
        let mut facets = vec![FacetKey::ProductType, FacetKey::Vendor];
        facets.extend(
            index
                .option_names(market.id())
                .await?
                .into_iter()
                .take(DEFAULT_OPTION_FACETS)
                .map(FacetKey::Option),
        );
        facets.push(FacetKey::Tag);
        (facets, BTreeMap::new())
    } else {
        let facets = settings.facets().iter().map(|f| f.key.clone()).collect();
        let labels = settings
            .facets()
            .iter()
            .filter_map(|f| f.label.clone().map(|l| (f.key.clone(), l)))
            .collect();
        (facets, labels)
    };

    let query = SearchQuery {
        market: market.id(),
        terms,
        filters,
        price: PriceBounds {
            min: input.price_min,
            max: input.price_max,
        },
        available: input.available,
        sort: input.sort,
        group: input.group,
        facets,
        offset: input.offset,
        limit: input.limit,
    };
    let results = index.search(&query).await?;
    Ok(Found {
        market,
        results,
        labels,
    })
}

/// `key:value` filters, grouped by key in the order keys first appear.
fn parse_filters(raw: &[String]) -> Result<Vec<Filter>, SearchError> {
    let mut filters: Vec<Filter> = Vec::new();
    for text in raw {
        let (key, value) = text
            .split_once(':')
            .filter(|(_, v)| !v.trim().is_empty())
            .ok_or_else(|| SearchError::InvalidFilter(text.clone()))?;
        let key: FacetKey = key.parse().map_err(SearchError::UnknownFacet)?;
        let value = value.trim().to_string();
        match filters.iter_mut().find(|f| f.key == key) {
            Some(filter) if !filter.values.contains(&value) => filter.values.push(value),
            Some(_) => {}
            None => filters.push(Filter {
                key,
                values: vec![value],
            }),
        }
    }
    Ok(filters)
}

/// The store's settings, or the defaults when none are saved.
pub async fn get_search_settings<S: HasSearchSettings>(
    ctx: &Ctx,
    ports: &S,
) -> Result<SearchSettings, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(ports.search_settings().get().await?.unwrap_or_default())
}

/// Changes only what's `Some`, like a JSON Merge Patch.
#[derive(Debug, Clone, Default)]
pub struct UpdateSearchSettings {
    pub split_by: Option<Vec<String>>,
    /// Facet keys as strings (`option.color`) and labels.
    pub facets: Option<Vec<(String, Option<String>)>>,
}

/// Changes the settings (search settings are how products are presented, so
/// `products:write`). Changing the splits rebuilds every document, through the
/// `search_settings.changed` event.
pub async fn update_search_settings<S: HasSearchSettings>(
    ctx: &Ctx,
    ports: &S,
    input: UpdateSearchSettings,
) -> Result<SearchSettings, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let stored = ports.search_settings().get().await?;
    let is_new = stored.is_none();
    let mut settings = stored.unwrap_or_default();
    if let Some(names) = input.split_by {
        settings.set_split_by(names)?;
    }
    if let Some(facets) = input.facets {
        let facets = facets
            .into_iter()
            .map(|(key, label)| {
                Ok(FacetSetting {
                    key: key.parse().map_err(SearchError::UnknownFacet)?,
                    label,
                })
            })
            .collect::<Result<Vec<_>, SearchError>>()?;
        settings.set_facets(facets)?;
    }
    if is_new {
        ports.search_settings().insert(&mut settings).await?;
    } else {
        ports.search_settings().update(&mut settings).await?;
    }
    Ok(settings)
}

/// Queues a rebuild of every document.
pub async fn reindex<S: HasSearchQueue>(ctx: &Ctx, ports: &S) -> Result<(), CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    Ok(queue_all(ports).await?)
}

/// How the index is doing: what's waiting to be rebuilt, and how much it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchStatus {
    pub queued: u64,
    pub documents: u64,
}

pub async fn search_status<S: HasSearchQueue, I: SearchIndex>(
    ctx: &Ctx,
    ports: &S,
    index: &I,
) -> Result<SearchStatus, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(SearchStatus {
        queued: ports.search_queue().queued().await?,
        documents: index.count().await?,
    })
}
