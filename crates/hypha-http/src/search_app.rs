//! The store's search app as a search engine (`search.query`): `/storefront/search` asks
//! the app, then shows Hypha's own documents for the products and variants it found (priced
//! and stocked as carts are, only what the storefront's channel sells), so an app chooses
//! and orders hits but can't put a price or a product on the page. The point falls back:
//! when the app fails, the built-in engine answers and the failure is logged (and in the
//! app's call log).
//!
//! Writes, option names and counts are always the built-in engine's: it stays in sync (it's
//! the fallback), and an app keeps its own index from webhooks.

use std::collections::{BTreeMap, BTreeSet};

use hypha_apps::{HasApps, HasExtensionEndpoints};
use hypha_core::{AppKey, ChannelId, MarketId, ProductId};
use hypha_extensions::{
    Answered, ExtensionPoint, HasExtensionCalls, SearchFilter, SearchGroup, SearchPriceRange,
    SearchQuery as SearchQueryPoint, SearchQueryRequest, SearchQueryResponse, SearchSort,
    apply_policy, call,
};
use hypha_markets::MarketRepo;
use hypha_search::{SyncPorts, documents_for};
use hypha_search_index::{
    FacetCounts, FacetKey, FacetValue, Group, Hit, IndexError, SearchDocument, SearchIndex,
    SearchQuery, SearchResults, Sort,
};

use crate::{
    app_endpoints::{AppEndpoints, HostAppEndpoints},
    extensions::HostExtensions,
    state::HostEnv,
};

/// The engine a storefront search uses: the store's search app when one is set, the
/// built-in engine otherwise and whenever the app fails.
pub struct AppSearch<'a, P, I> {
    pub built_in: I,
    pub ports: &'a P,
    /// The store's search app (`SearchSettings::search_app`).
    pub app: Option<AppKey>,
    /// The words as the shopper typed them (the query only has their terms).
    pub words: Option<String>,
    pub endpoints: HostAppEndpoints<'a, P>,
    pub caller: HostExtensions<'a>,
    pub env: &'a HostEnv,
}

impl<P, I> AppSearch<'_, P, I>
where
    P: SyncPorts + HasApps + HasExtensionEndpoints + HasExtensionCalls,
    I: SearchIndex,
{
    /// The app's results, or why there are none (for the log).
    async fn ask(&self, app: &AppKey, query: &SearchQuery) -> Result<SearchResults, String> {
        let endpoint = match self.endpoints.endpoint(app, SearchQueryPoint::NAME).await {
            Ok(Some(found)) => found,
            Ok(None) => return Err("it doesn't answer search.query (any more)".to_string()),
            Err(e) => return Err(e.to_string()),
        };
        let market = self
            .ports
            .markets()
            .get(query.market)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("market {} is gone", query.market))?;
        let request = wire_request(query, market.currency(), self.words.as_deref());
        // A search changes nothing: a fresh key per call, for the app's logs.
        let key = format!(
            "search_{}",
            hypha_core::IdGenerator::new_uuid(self.env).simple()
        );
        let answered = apply_policy::<SearchQueryPoint>(
            call::<SearchQueryPoint>(
                &self.caller,
                self.ports.extension_calls(),
                self.env,
                &endpoint.as_endpoint(),
                &request,
                &key,
                None,
            )
            .await,
        )
        .map_err(|e| e.to_string())?;
        let answer = match answered {
            Answered::App(answer) => answer,
            Answered::Fallback(e) => return Err(e.to_string()),
        };
        results(self.ports, market, query, answer)
            .await
            .map_err(|e| e.to_string())
    }
}

impl<P, I> SearchIndex for AppSearch<'_, P, I>
where
    P: SyncPorts + HasApps + HasExtensionEndpoints + HasExtensionCalls,
    I: SearchIndex,
{
    async fn replace(
        &self,
        market: MarketId,
        products: &[ProductId],
        documents: &[SearchDocument],
    ) -> Result<(), IndexError> {
        self.built_in.replace(market, products, documents).await
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError> {
        let Some(app) = &self.app else {
            return self.built_in.search(query).await;
        };
        // An app indexes products from webhooks and can't know what's on sale: that's
        // pricing, which only the built-in documents carry.
        let on_sale = query.filters.iter().any(|f| f.key == FacetKey::OnSale)
            || query.facets.contains(&FacetKey::OnSale);
        if on_sale {
            return self.built_in.search(query).await;
        }
        match self.ask(app, query).await {
            Ok(results) => Ok(results),
            Err(why) => {
                tracing::warn!(%app, point = SearchQueryPoint::NAME, %why,
                    "search app failed; the built-in engine answers");
                self.built_in.search(query).await
            }
        }
    }

    async fn option_names(
        &self,
        market: MarketId,
        channel: Option<ChannelId>,
    ) -> Result<Vec<String>, IndexError> {
        self.built_in.option_names(market, channel).await
    }

    async fn count(&self) -> Result<u64, IndexError> {
        self.built_in.count().await
    }
}

/// A checked query as the point sends it.
fn wire_request(
    q: &SearchQuery,
    currency: hypha_core::Currency,
    words: Option<&str>,
) -> SearchQueryRequest {
    SearchQueryRequest {
        query: words.map(str::trim).unwrap_or_default().to_string(),
        market: q.market,
        channel: q.channel,
        currency,
        category: q.category,
        filters: q
            .filters
            .iter()
            .map(|f| SearchFilter {
                facet: f.key.to_string(),
                values: f.values.clone(),
            })
            .collect(),
        price: SearchPriceRange {
            min: q.price.min,
            max: q.price.max,
        },
        available: q.available,
        sort: match q.sort {
            Sort::Relevance => SearchSort::Relevance,
            Sort::Newest => SearchSort::Newest,
            Sort::PriceAsc => SearchSort::PriceAsc,
            Sort::PriceDesc => SearchSort::PriceDesc,
            Sort::Title => SearchSort::Title,
        },
        group: match q.group {
            Group::Listing => SearchGroup::Listing,
            Group::Product => SearchGroup::Product,
            Group::Variant => SearchGroup::Variant,
        },
        facets: q.facets.iter().map(ToString::to_string).collect(),
        offset: q.offset,
        limit: q.limit,
    }
}

/// The app's hits as results: each hit's document as Hypha builds it now (the hit's
/// variant, else the product's first), dropped when the product isn't sold in the
/// market or published to the storefront's channel. The facets asked for, in order
/// (an app's count for a facet not asked for was refused by the point's check); no
/// price range, which an app doesn't answer.
async fn results<P: SyncPorts>(
    ports: &P,
    market: hypha_markets::Market,
    query: &SearchQuery,
    answer: SearchQueryResponse,
) -> Result<SearchResults, hypha_search::SyncError> {
    let mut products: Vec<ProductId> = Vec::new();
    let mut seen = BTreeSet::new();
    for hit in &answer.hits {
        if seen.insert(hit.product) {
            products.push(hit.product);
        }
    }
    let mut by_product: BTreeMap<ProductId, Vec<SearchDocument>> = BTreeMap::new();
    for document in documents_for(ports, market, &products).await? {
        let shown = query.channel.is_none_or(|c| document.channels.contains(&c));
        if shown {
            by_product
                .entry(document.product)
                .or_default()
                .push(document);
        }
    }
    let hits = answer
        .hits
        .iter()
        .filter_map(|hit| {
            let documents = by_product.get(&hit.product)?;
            let document = match hit.variant {
                Some(variant) => documents.iter().find(|d| d.variant == variant)?,
                None => documents.iter().min_by_key(|d| d.position)?,
            };
            Some(Hit {
                document: document.clone(),
                matched: 1,
            })
        })
        .collect();
    let facets = query
        .facets
        .iter()
        .map(|key| FacetCounts {
            key: key.clone(),
            values: answer
                .facets
                .iter()
                .find(|f| f.facet.parse::<FacetKey>().as_ref() == Ok(key))
                .map(|f| {
                    f.values
                        .iter()
                        .map(|v| FacetValue {
                            value: v.value.clone(),
                            count: v.count,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    Ok(SearchResults {
        hits,
        total: answer.total,
        facets,
        price: None,
    })
}
