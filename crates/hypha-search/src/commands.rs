//! Searching a market (design note 0019, SE6): the shopper's country picks the market.
//! Changing the settings checks custom field facets here (design note 0033, §9: the
//! leaf can't see definitions), then is `hypha_search_index`'s, as are reindex and
//! status.

use std::collections::BTreeMap;

use hypha_categories::{CategoryRepo, CategoryTree, HasCategories};
use hypha_core::{
    AppPoints, AuthError, CategoryId, CountryCode, CountryError, Ctx, ErrorCode, RepoError, Scope,
    optional_text,
};
use hypha_custom_fields::{DefinitionRepo, HasCustomFieldDefinitions};
use hypha_markets::{HasMarkets, Market, MarketRepo};
use hypha_search_index::{
    FacetKey, Filter, Group, HasSearchSettings, IndexError, MAX_DEPTH, MAX_LIMIT, PriceBounds,
    SearchError, SearchIndex, SearchQuery, SearchResults, SearchSettings, SearchSettingsRepo, Sort,
    UpdateSearchSettings, terms,
};

use crate::fields::{choice_labels, facet_field};
use thiserror::Error;

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
    /// No such category, or (for a storefront) one hidden itself or under a hidden one.
    #[error("category {0} not found")]
    CategoryNotFound(CategoryId),
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
            CommandError::CategoryNotFound(_) => "category_not_found",
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
    /// Only products in this category or below it (design note 0022, CA3).
    pub category: Option<CategoryId>,
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
    /// The value labels of `choice` and `boolean` field facets in `results`, by stored
    /// value: a choice's label, "Yes"/"No" (design note 0033, §9: filters use values).
    pub value_labels: BTreeMap<FacetKey, BTreeMap<String, String>>,
}

/// How many default option facets a search counts (the most used options).
pub const DEFAULT_OPTION_FACETS: usize = 5;

/// Searches the market serving `input.country` (design note 0019, SE6). The facets
/// counted are the merchant's (`SearchSettings::facets`), else product type, vendor,
/// the market's most used options and tags.
pub async fn search<
    S: HasMarkets + HasSearchSettings + HasCategories + HasCustomFieldDefinitions,
    I: SearchIndex,
>(
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
    // A page may start at most `MAX_DEPTH - 1` results in; one that would end past
    // the limit is cut short there, so following `next_cursor` never fails.
    if input.offset >= MAX_DEPTH {
        return Err(SearchError::TooDeep { max: MAX_DEPTH }.into());
    }
    let limit = input.limit.min(MAX_DEPTH - input.offset);

    let settings = ports.search_settings().get().await?.unwrap_or_default();
    let (facets, labels) = if settings.facets().is_empty() {
        let mut facets = vec![FacetKey::ProductType, FacetKey::Vendor];
        facets.extend(
            index
                .option_names(market.id(), ctx.channel())
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

    // A storefront (a caller with a channel) sees only shown categories (CA4); staff
    // may filter by any.
    if let Some(category) = input.category {
        let tree = CategoryTree::new(ports.categories().list_all().await?);
        let usable = match ctx.channel() {
            Some(_) => tree.is_shown(category),
            None => tree.contains(category),
        };
        if !usable {
            return Err(CommandError::CategoryNotFound(category));
        }
    }

    let query = SearchQuery {
        market: market.id(),
        channel: ctx.channel(),
        category: input.category,
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
        limit,
    };
    let results = index.search(&query).await?;
    let value_labels = choice_labels(ports, &query.facets).await?;
    Ok(Found {
        market,
        results,
        labels,
        value_labels,
    })
}

impl From<hypha_search_index::CommandError> for CommandError {
    fn from(e: hypha_search_index::CommandError) -> Self {
        use hypha_search_index::CommandError as E;
        match e {
            E::Auth(e) => CommandError::Auth(e),
            E::Search(e) => CommandError::Search(e),
            E::Repo(e) => CommandError::Repo(e),
            E::Index(e) => CommandError::Index(e),
        }
    }
}

/// Changes the search settings (`hypha_search_index::update_search_settings`), after
/// checking each `field.<namespace>.<key>` facet names a field that may be one
/// (`facet_field`: a filterable product or variant field a storefront reads, of a type
/// with discrete values), else `facet_field_not_allowed`.
pub async fn update_search_settings<S: HasSearchSettings + HasCustomFieldDefinitions>(
    ctx: &Ctx,
    ports: &S,
    apps: &impl AppPoints,
    input: UpdateSearchSettings,
) -> Result<SearchSettings, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let fields: Vec<String> = input
        .facets
        .iter()
        .flatten()
        .filter_map(|(key, _)| match key.parse() {
            Ok(FacetKey::Field(field)) => Some(field),
            _ => None,
        })
        .collect();
    if !fields.is_empty() {
        let definitions = ports.custom_field_definitions().list(None).await?;
        for field in fields {
            let allowed = definitions
                .iter()
                .any(|d| facet_field(d) && d.field_key().to_string() == field);
            if !allowed {
                return Err(SearchError::FacetFieldNotAllowed(field).into());
            }
        }
    }
    Ok(hypha_search_index::update_search_settings(ctx, ports, apps, input).await?)
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
