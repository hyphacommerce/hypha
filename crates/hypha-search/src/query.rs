//! What a search asks and answers (design note 0019, section 4), engine-neutral.

use std::{fmt, str::FromStr};

use hypha_core::{MarketId, Money};

use crate::SearchDocument;

/// What a facet counts, and a filter narrows by. As a string (the API's form):
/// `option.<name>` (name lowercased), `vendor`, `product_type`, `tag`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FacetKey {
    /// An option, by its name ignoring case ("Color" and "color" are one facet).
    Option(String),
    Vendor,
    ProductType,
    Tag,
}

impl FacetKey {
    const OPTION: &'static str = "option.";
}

impl fmt::Display for FacetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FacetKey::Option(name) => write!(f, "{}{name}", Self::OPTION),
            FacetKey::Vendor => f.write_str("vendor"),
            FacetKey::ProductType => f.write_str("product_type"),
            FacetKey::Tag => f.write_str("tag"),
        }
    }
}

impl FromStr for FacetKey {
    /// The text that isn't a key.
    type Err = String;

    /// The `Display` form; an option's name is lowercased and must not be blank.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "vendor" => Ok(FacetKey::Vendor),
            "product_type" => Ok(FacetKey::ProductType),
            "tag" => Ok(FacetKey::Tag),
            _ => match s.strip_prefix(Self::OPTION).map(str::trim) {
                Some(name) if !name.is_empty() => Ok(FacetKey::Option(name.to_lowercase())),
                _ => Err(s.to_string()),
            },
        }
    }
}

/// Documents with any of `values` for `key` (exact values, as facets list them).
/// Filters on different keys must all match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub key: FacetKey,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Best match first; without terms, `Newest`.
    #[default]
    Relevance,
    Newest,
    PriceAsc,
    PriceDesc,
    /// By title, ignoring case.
    Title,
}

/// What one hit stands for (design note 0019, SE4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Group {
    /// A product, or a product split by the merchant's options.
    #[default]
    Listing,
    Product,
    /// Every variant on its own.
    Variant,
}

/// Inclusive bounds on the price, in the market currency's minor units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PriceBounds {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

/// A checked query (the `search` command builds it from a request).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub market: MarketId,
    /// From `terms::terms`; empty lists everything.
    pub terms: Vec<String>,
    pub filters: Vec<Filter>,
    pub price: PriceBounds,
    pub available: Option<bool>,
    pub sort: Sort,
    pub group: Group,
    /// The facets to count, in the order to return them.
    pub facets: Vec<FacetKey>,
    pub offset: u32,
    pub limit: u32,
}

/// One result: the representative document of its group, and how many of the group's
/// documents matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub document: SearchDocument,
    pub matched: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetValue {
    pub value: String,
    /// Groups (what the shopper sees), not documents.
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetCounts {
    pub key: FacetKey,
    /// Most groups first, then by value; at most `MAX_FACET_VALUES`.
    pub values: Vec<FacetValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResults {
    pub hits: Vec<Hit>,
    /// Groups matched, over every page.
    pub total: u64,
    /// One per asked-for facet, in order (empty values if none).
    pub facets: Vec<FacetCounts>,
    /// The lowest and highest price among the matches, ignoring the price bounds (for a
    /// price slider); `None` when nothing matches.
    pub price: Option<(Money, Money)>,
}

/// The most values a facet lists.
pub const MAX_FACET_VALUES: usize = 100;
/// The deepest a search pages: `offset + limit` at most this (engines page by offset).
pub const MAX_DEPTH: u32 = 1000;
/// The most hits per page.
pub const MAX_LIMIT: u32 = 100;
