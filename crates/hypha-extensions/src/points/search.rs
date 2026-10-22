//! `search.query` v1: `/storefront/search` answered by the store's search app
//! (Meilisearch's) instead of the built-in engine. Hypha sends the shopper's query as the
//! storefront gave it, already checked; the app answers with IDs, which Hypha loads, prices
//! and stocks itself, so an app can't put a price or a product the shopper may not see on
//! the page.

use core::time::Duration;

use hypha_core::{CategoryId, ChannelId, Currency, MarketId, ProductId, Scope, VariantId};
use serde::{Deserialize, Serialize};

use crate::{ExtensionPoint, OnFailure};

/// A storefront search, answered by the store's search app.
pub struct SearchQuery;

impl ExtensionPoint for SearchQuery {
    const NAME: &'static str = "search.query";
    const VERSION: u16 = 1;
    const TIMEOUT: Duration = Duration::from_secs(3);
    /// The built-in engine answers instead.
    const ON_FAILURE: OnFailure = OnFailure::Fallback;
    /// It answers with products and variants, so it reads them.
    const SCOPE: Scope = Scope::ProductsRead;
    type Request = SearchQueryRequest;
    type Response = SearchQueryResponse;

    /// No more hits than asked for, and facet counts only for the facets asked.
    fn check(request: &SearchQueryRequest, response: &SearchQueryResponse) -> Result<(), String> {
        if response.hits.len() > request.limit as usize {
            return Err(format!(
                "{} hits, but at most {} were asked for",
                response.hits.len(),
                request.limit
            ));
        }
        if let Some(extra) = response
            .facets
            .iter()
            .find(|f| !request.facets.contains(&f.facet))
        {
            return Err(format!("facet {:?} wasn't asked for", extra.facet));
        }
        Ok(())
    }
}

/// What the shopper searched for, in one market and channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchQueryRequest {
    /// The words as the shopper typed them; empty lists everything that matches the
    /// filters.
    pub query: String,
    /// The shopper's market: what's sold to them, at its prices.
    pub market: MarketId,
    /// The storefront's sales channel: only products published to it. Absent for a
    /// staff search, which sees every channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub channel: Option<ChannelId>,
    /// The market's currency, which `price` bounds are in.
    pub currency: Currency,
    /// Only products in this category or one below it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub category: Option<CategoryId>,
    /// Products with any of each filter's values; filters on different facets must all
    /// match.
    #[serde(default)]
    pub filters: Vec<SearchFilter>,
    /// Inclusive price bounds, in the currency's minor units.
    #[serde(default)]
    pub price: SearchPriceRange,
    /// Only what's in stock (`true`), out of stock (`false`), or either (absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub available: Option<bool>,
    pub sort: SearchSort,
    /// What one hit stands for.
    pub group: SearchGroup,
    /// The facets to count (`vendor`, `product_type`, `tag`, `option.<name>`), in the
    /// order to answer them.
    #[serde(default)]
    pub facets: Vec<String>,
    /// Hits to skip, then at most `limit` (1 to 100) to answer.
    pub offset: u32,
    pub limit: u32,
}

/// A facet's accepted values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchFilter {
    /// `vendor`, `product_type`, `tag` or `option.<name>` (name lowercased).
    pub facet: String,
    pub values: Vec<String>,
}

/// Inclusive bounds on a price, in minor units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchPriceRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub max: Option<i64>,
}

/// The order of the hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    /// Best match first; without words, newest.
    Relevance,
    Newest,
    PriceAsc,
    PriceDesc,
    /// By title, ignoring case.
    Title,
}

/// What one hit stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SearchGroup {
    /// A product, or a product split by the merchant's options.
    Listing,
    Product,
    /// Every variant on its own.
    Variant,
}

/// The app's answer: one page of hits, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchQueryResponse {
    pub hits: Vec<SearchHit>,
    /// One per asked-for facet, in the asked order; a facet left out counts nothing.
    #[serde(default)]
    pub facets: Vec<SearchFacetCounts>,
    /// Hits over every page.
    pub total: u64,
}

/// One hit: a product, and the variant that stands for it when the group is a variant
/// or a listing split by options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchHit {
    pub product: ProductId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub variant: Option<VariantId>,
}

/// A facet's values with how many hits have each, most first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchFacetCounts {
    pub facet: String,
    pub values: Vec<SearchFacetValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchFacetValue {
    pub value: String,
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn request() -> SearchQueryRequest {
        SearchQueryRequest {
            query: "blue mug".to_string(),
            market: MarketId::from_uuid(Uuid::from_u128(1)),
            channel: Some(ChannelId::from_uuid(Uuid::from_u128(2))),
            currency: Currency::EUR,
            category: None,
            filters: vec![],
            price: SearchPriceRange::default(),
            available: None,
            sort: SearchSort::Relevance,
            group: SearchGroup::Listing,
            facets: vec!["vendor".to_string()],
            offset: 0,
            limit: 2,
        }
    }

    fn answer(value: serde_json::Value) -> SearchQueryResponse {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn hits_are_ids_in_order() {
        let parsed = answer(json!({
            "hits": [{"product": "prod_00000000000000000000000000000003"},
                     {"product": "prod_00000000000000000000000000000004",
                      "variant": "var_00000000000000000000000000000005"}],
            "facets": [{"facet": "vendor", "values": [{"value": "Acme", "count": 2}]}],
            "total": 9
        }));
        assert_eq!(parsed.hits[0].variant, None);
        assert_eq!(parsed.total, 9);
        assert_eq!(SearchQuery::check(&request(), &parsed), Ok(()));
    }

    #[test]
    fn more_hits_than_asked_for_are_refused() {
        let parsed = answer(json!({
            "hits": [{"product": "prod_00000000000000000000000000000003"},
                     {"product": "prod_00000000000000000000000000000003"},
                     {"product": "prod_00000000000000000000000000000003"}],
            "total": 3
        }));
        assert_eq!(
            SearchQuery::check(&request(), &parsed),
            Err("3 hits, but at most 2 were asked for".to_string())
        );
    }

    #[test]
    fn a_facet_not_asked_for_is_refused() {
        let parsed = answer(json!({"hits": [], "total": 0,
            "facets": [{"facet": "tag", "values": []}]}));
        assert_eq!(
            SearchQuery::check(&request(), &parsed),
            Err("facet \"tag\" wasn't asked for".to_string())
        );
    }

    #[test]
    fn the_request_leaves_out_what_isnt_set() {
        let sent = serde_json::to_value(request()).unwrap();
        assert_eq!(sent.get("category"), None);
        assert_eq!(sent["price"], json!({}));
        assert_eq!(sent["sort"], "relevance");
    }
}
