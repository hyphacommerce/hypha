//! What a search means, as a plain function over documents: the in-memory engine runs it,
//! and the `search_index` contract holds every engine to it, relevance ranking included. An
//! engine with a ranking of its own (Meilisearch) will need an allowance for relevance
//! order and facet counts.
//!
//! 1. **Match:** the market's documents whose terms contain every query term as a
//!    prefix (`terms::matches_all`), within the price bounds, availability and every
//!    filter (values of one key OR, keys AND; `on_sale` is one such key).
//! 2. **Group:** by the query's group key; a group's representative is its first
//!    document by the sort (lowest price for `PriceAsc`, highest for `PriceDesc`,
//!    best score for `Relevance`), then by `position`.
//! 3. **Order** groups by their representative: the sort's key, then newest product
//!    (created, then product ID, descending), then listing key and position.
//! 4. **Facets** count groups per value, ignoring the facet's own filter (so choosing
//!    "Red" still shows "Blue"); the price range ignores the price bounds.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use hypha_core::{ChannelId, MarketId};

use crate::{
    FacetCounts, FacetKey, FacetValue, Group, Hit, MAX_FACET_VALUES, SearchDocument, SearchQuery,
    SearchResults, Sort, terms,
};

/// The weight of a term found in a field (title 10, vendor/type/tags/SKU/options 4,
/// description 1); a document's score is the sum over query terms of the best.
pub fn score(document: &SearchDocument, query_terms: &[String]) -> u32 {
    let weighted = document.terms();
    query_terms
        .iter()
        .map(|q| {
            let has = |field: &[String]| field.iter().any(|t| t.starts_with(q.as_str()));
            if has(&weighted.a) {
                10
            } else if has(&weighted.b) {
                4
            } else if has(&weighted.d) {
                1
            } else {
                0
            }
        })
        .sum()
}

pub fn group_key(document: &SearchDocument, group: Group) -> String {
    match group {
        Group::Listing => document.listing.clone(),
        Group::Product => document.product.to_string(),
        Group::Variant => document.variant.to_string(),
    }
}

/// The option names (lowercased) of `market`'s documents, those published to `channel`
/// if there is one: most documents first, then by name. What `SearchIndex::option_names`
/// returns, and so the default facets.
pub fn option_names(
    documents: &[SearchDocument],
    market: MarketId,
    channel: Option<ChannelId>,
) -> Vec<String> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for doc in documents
        .iter()
        .filter(|d| d.market == market)
        .filter(|d| channel.is_none_or(|c| d.channels.contains(&c)))
    {
        for (name, _) in &doc.options {
            *counts.entry(name.to_lowercase()).or_default() += 1;
        }
    }
    let mut names: Vec<(String, u64)> = counts.into_iter().collect();
    names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    names.into_iter().map(|(name, _)| name).collect()
}

/// Runs `query` over `documents` (every market's; the query's is picked out).
pub fn search(documents: &[SearchDocument], query: &SearchQuery) -> SearchResults {
    let in_market: Vec<&SearchDocument> = documents
        .iter()
        .filter(|d| d.market == query.market)
        .filter(|d| query.channel.is_none_or(|c| d.channels.contains(&c)))
        .filter(|d| query.category.is_none_or(|c| d.categories.contains(&c)))
        .filter(|d| query.terms.is_empty() || terms::matches_all(&query.terms, &d.terms().all()))
        .filter(|d| query.available.is_none_or(|a| d.available == a))
        .collect();
    let in_price = |d: &&SearchDocument| {
        query.price.min.is_none_or(|min| d.price.amount() >= min)
            && query.price.max.is_none_or(|max| d.price.amount() <= max)
    };
    let passes = |d: &SearchDocument, except: Option<&FacetKey>| {
        query
            .filters
            .iter()
            .filter(|f| Some(&f.key) != except)
            .all(|f| {
                let key = f.key.to_string();
                d.facet_values()
                    .iter()
                    .any(|(k, v)| *k == key && f.values.contains(v))
            })
    };

    let matched: Vec<&SearchDocument> = in_market
        .iter()
        .copied()
        .filter(in_price)
        .filter(|d| passes(d, None))
        .collect();

    // Groups, each with its documents in representative order.
    let sort = effective_sort(query);
    let mut groups: BTreeMap<String, Vec<&SearchDocument>> = BTreeMap::new();
    for doc in &matched {
        groups
            .entry(group_key(doc, query.group))
            .or_default()
            .push(doc);
    }
    let mut representatives: Vec<(&SearchDocument, u32)> = groups
        .values_mut()
        .map(|docs| {
            docs.sort_by(|a, b| within_group(a, b, sort, &query.terms));
            (docs[0], docs.len() as u32)
        })
        .collect();
    representatives.sort_by(|(a, _), (b, _)| between_groups(a, b, sort, &query.terms));
    let total = representatives.len() as u64;
    let hits = representatives
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .map(|(doc, matched)| Hit {
            document: doc.clone(),
            matched,
        })
        .collect();

    let facets = query
        .facets
        .iter()
        .map(|key| {
            let key_text = key.to_string();
            let mut counts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for doc in in_market
                .iter()
                .copied()
                .filter(in_price)
                .filter(|d| passes(d, Some(key)))
            {
                for (k, value) in doc.facet_values() {
                    if k == key_text {
                        counts
                            .entry(value)
                            .or_default()
                            .insert(group_key(doc, query.group));
                    }
                }
            }
            let mut values: Vec<FacetValue> = counts
                .into_iter()
                .map(|(value, groups)| FacetValue {
                    value,
                    count: groups.len() as u64,
                })
                .collect();
            values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
            values.truncate(MAX_FACET_VALUES);
            FacetCounts {
                key: key.clone(),
                values,
            }
        })
        .collect();

    let priced: Vec<&SearchDocument> = in_market
        .iter()
        .copied()
        .filter(|d| passes(d, None))
        .collect();
    let price = priced
        .iter()
        .map(|d| d.price)
        .min_by_key(|p| p.amount())
        .zip(priced.iter().map(|d| d.price).max_by_key(|p| p.amount()));

    SearchResults {
        hits,
        total,
        facets,
        price,
    }
}

/// `Relevance` without terms is `Newest`.
pub fn effective_sort(query: &SearchQuery) -> Sort {
    match query.sort {
        Sort::Relevance if query.terms.is_empty() => Sort::Newest,
        sort => sort,
    }
}

fn within_group(a: &SearchDocument, b: &SearchDocument, sort: Sort, q: &[String]) -> Ordering {
    let by_sort = match sort {
        Sort::PriceAsc => a.price.amount().cmp(&b.price.amount()),
        Sort::PriceDesc => b.price.amount().cmp(&a.price.amount()),
        Sort::Relevance => score(b, q).cmp(&score(a, q)),
        Sort::Newest | Sort::Title => Ordering::Equal,
    };
    by_sort
        .then_with(|| a.product.cmp(&b.product))
        .then_with(|| a.position.cmp(&b.position))
}

fn between_groups(a: &SearchDocument, b: &SearchDocument, sort: Sort, q: &[String]) -> Ordering {
    let by_sort = match sort {
        Sort::PriceAsc => a.price.amount().cmp(&b.price.amount()),
        Sort::PriceDesc => b.price.amount().cmp(&a.price.amount()),
        Sort::Relevance => score(b, q).cmp(&score(a, q)),
        Sort::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        Sort::Newest => Ordering::Equal,
    };
    by_sort
        .then_with(|| b.created_at.cmp(&a.created_at))
        .then_with(|| b.product.cmp(&a.product))
        .then_with(|| a.listing.cmp(&b.listing))
        .then_with(|| a.position.cmp(&b.position))
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, Money, ProductId, Timestamp, VariantId};
    use uuid::Uuid;

    use super::*;
    use crate::{Filter, PriceRange};

    const MATERIAL: &str = "custom.material";

    fn doc(product: u128, variant: u128, fields: &[(&str, &str)]) -> SearchDocument {
        let price = Money::new(1000, Currency::EUR).unwrap();
        let product = ProductId::from_uuid(Uuid::from_u128(product));
        SearchDocument {
            market: MarketId::from_uuid(Uuid::from_u128(1)),
            product,
            variant: VariantId::from_uuid(Uuid::from_u128(variant)),
            listing: product.to_string(),
            channels: vec![],
            categories: vec![],
            handle: "h".to_string(),
            title: "T".to_string(),
            variant_title: None,
            description: String::new(),
            vendor: None,
            product_type: None,
            tags: vec![],
            sku: None,
            options: vec![],
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            price,
            compare_at: None,
            on_sale: false,
            product_price: PriceRange::of(price),
            listing_price: PriceRange::of(price),
            available: true,
            image: None,
            created_at: Timestamp::from_unix_micros(0),
            position: 0,
        }
    }

    fn query(filters: Vec<Filter>, facets: Vec<FacetKey>) -> SearchQuery {
        SearchQuery {
            market: MarketId::from_uuid(Uuid::from_u128(1)),
            channel: None,
            category: None,
            terms: vec![],
            filters,
            price: Default::default(),
            available: None,
            sort: Sort::Newest,
            group: Group::Product,
            facets,
            offset: 0,
            limit: 10,
        }
    }

    fn field() -> FacetKey {
        FacetKey::Field(MATERIAL.to_string())
    }

    fn documents() -> Vec<SearchDocument> {
        vec![
            // A product of wool, one variant of it silk instead.
            doc(1, 11, &[(MATERIAL, "wool")]),
            doc(1, 12, &[(MATERIAL, "silk")]),
            // A list: one entry per item.
            doc(2, 21, &[(MATERIAL, "wool"), (MATERIAL, "linen")]),
            doc(3, 31, &[]),
        ]
    }

    #[test]
    fn a_field_filter_keeps_documents_with_any_of_its_values() {
        let found = search(
            &documents(),
            &query(
                vec![Filter {
                    key: field(),
                    values: vec!["linen".to_string(), "silk".to_string()],
                }],
                vec![],
            ),
        );
        let products: Vec<u128> = found
            .hits
            .iter()
            .map(|h| h.document.product.as_uuid().as_u128())
            .collect();
        assert_eq!(products, vec![2, 1]);
        // Product 1 matches through its silk variant only.
        assert_eq!(found.hits[1].matched, 1);
    }

    #[test]
    fn a_field_facet_counts_groups_per_value_ignoring_its_own_filter() {
        let found = search(
            &documents(),
            &query(
                vec![Filter {
                    key: field(),
                    values: vec!["silk".to_string()],
                }],
                vec![field()],
            ),
        );
        let counts: Vec<(String, u64)> = found.facets[0]
            .values
            .iter()
            .map(|v| (v.value.clone(), v.count))
            .collect();
        assert_eq!(
            counts,
            vec![
                ("wool".to_string(), 2),
                ("linen".to_string(), 1),
                ("silk".to_string(), 1),
            ]
        );
        assert_eq!(found.total, 1);
    }

    #[test]
    fn field_values_are_facet_values_under_their_key() {
        assert_eq!(
            doc(2, 21, &[(MATERIAL, "wool"), (MATERIAL, "linen")]).facet_values(),
            vec![
                ("on_sale".to_string(), "false".to_string()),
                ("field.custom.material".to_string(), "wool".to_string()),
                ("field.custom.material".to_string(), "linen".to_string()),
            ]
        );
    }
}
