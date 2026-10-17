//! The reference semantics (design note 0019, section 4), worked out by hand on the
//! testkit's fixture. Engines are held to the reference by the contract; this holds the
//! reference to the note.
//!
//! Market 1: Linen Shirt (1, oldest; split by colour: Red is S 3000 and M 3200 out of
//! stock, Blue is S 2800), Cotton Shirt (2; Red L and Green L, both 1500), Čaša Mug (3,
//! 900) and Linen Towel (4, 1200, out of stock), the last two created at once.

use hypha_core::{MarketId, Money, VariantId};
use hypha_search_index::{
    FacetKey, FacetValue, Filter, Group, PriceBounds, SearchQuery, SearchResults, Sort, reference,
    terms,
};
use hypha_testkit::search_fixture;
use uuid::Uuid;

fn v(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn query() -> SearchQuery {
    SearchQuery {
        market: MarketId::from_uuid(Uuid::from_u128(1)),
        channel: None,
        category: None,
        terms: vec![],
        filters: vec![],
        price: PriceBounds::default(),
        available: None,
        sort: Sort::Relevance,
        group: Group::Listing,
        facets: vec![],
        offset: 0,
        limit: 10,
    }
}

fn run(q: &SearchQuery) -> SearchResults {
    reference::search(&search_fixture(), q)
}

fn hits(results: &SearchResults) -> Vec<VariantId> {
    results.hits.iter().map(|h| h.document.variant).collect()
}

fn color(values: &[&str]) -> Filter {
    Filter {
        key: FacetKey::Option("color".to_string()),
        values: values.iter().map(|v| v.to_string()).collect(),
    }
}

fn values(pairs: &[(&str, u64)]) -> Vec<FacetValue> {
    pairs
        .iter()
        .map(|(value, count)| FacetValue {
            value: value.to_string(),
            count: *count,
        })
        .collect()
}

#[test]
fn listings_newest_first_with_ties_by_product_then_listing() {
    let results = run(&query());
    assert_eq!(results.total, 5);
    // Towel and mug share a time: the higher product ID first. The shirt's listings
    // by key: Blue before Red.
    assert_eq!(hits(&results), [v(41), v(31), v(21), v(13), v(11)]);
    let matched: Vec<u32> = results.hits.iter().map(|h| h.matched).collect();
    assert_eq!(matched, [1, 1, 2, 1, 2]);
}

#[test]
fn products_collapse_every_variant() {
    let mut q = query();
    q.group = Group::Product;
    let results = run(&q);
    assert_eq!(results.total, 4);
    assert_eq!(hits(&results), [v(41), v(31), v(21), v(11)]);
}

#[test]
fn variants_stand_alone() {
    let mut q = query();
    q.group = Group::Variant;
    assert_eq!(run(&q).total, 7);
}

#[test]
fn by_price_a_listing_shows_its_cheapest() {
    let mut q = query();
    q.sort = Sort::PriceAsc;
    // Cotton's two 1500 variants tie: the first by position.
    assert_eq!(hits(&run(&q)), [v(31), v(41), v(21), v(13), v(11)]);
    q.sort = Sort::PriceDesc;
    assert_eq!(hits(&run(&q)), [v(12), v(13), v(21), v(41), v(31)]);
}

#[test]
fn by_title_ignoring_case() {
    let mut q = query();
    q.sort = Sort::Title;
    // Cotton, Linen Shirt (Blue, Red), Linen Towel, Čaša (beyond z).
    assert_eq!(hits(&run(&q)), [v(21), v(13), v(11), v(41), v(31)]);
}

#[test]
fn relevance_weighs_titles_over_descriptions() {
    let mut q = query();
    q.terms = terms::terms("lin");
    // Titles (towel, shirt) before the cotton shirt's description; ties newest first.
    assert_eq!(hits(&run(&q)), [v(41), v(13), v(11), v(21)]);
}

#[test]
fn every_term_must_match_as_a_prefix() {
    let mut q = query();
    q.terms = terms::terms("linen shirt");
    assert_eq!(hits(&run(&q)), [v(13), v(11), v(21)]);
    q.terms = terms::terms("ČAŠ");
    assert_eq!(hits(&run(&q)), [v(31)]);
    q.terms = terms::terms("ls-red");
    q.group = Group::Variant;
    assert_eq!(hits(&run(&q)), [v(11), v(12)]);
}

#[test]
fn facets_count_groups_and_ignore_their_own_filter() {
    let mut q = query();
    q.filters = vec![color(&["Red"])];
    q.facets = vec![
        FacetKey::Option("color".to_string()),
        FacetKey::Option("size".to_string()),
        FacetKey::Vendor,
    ];
    let results = run(&q);
    assert_eq!(results.total, 2);
    assert_eq!(hits(&results), [v(21), v(11)]);
    // Colour ignores the colour filter, so Blue and Green still show.
    assert_eq!(
        results.facets[0].values,
        values(&[("Red", 2), ("Blue", 1), ("Green", 1)])
    );
    // Size counts within red: one listing each.
    assert_eq!(
        results.facets[1].values,
        values(&[("L", 1), ("M", 1), ("S", 1)])
    );
    assert_eq!(
        results.facets[2].values,
        values(&[("Acme", 1), ("Basics", 1)])
    );
}

#[test]
fn values_of_a_key_or_and_keys_and() {
    let mut q = query();
    q.filters = vec![color(&["Red", "Blue"])];
    assert_eq!(run(&q).total, 3);
    q.filters.push(Filter {
        key: FacetKey::Vendor,
        values: vec!["Acme".to_string()],
    });
    assert_eq!(hits(&run(&q)), [v(13), v(11)]);
}

#[test]
fn price_bounds_are_inclusive_and_the_range_ignores_them() {
    let mut q = query();
    q.price = PriceBounds {
        min: Some(1500),
        max: Some(3000),
    };
    let results = run(&q);
    assert_eq!(hits(&results), [v(21), v(13), v(11)]);
    let eur = |n| Money::new(n, hypha_core::Currency::EUR).unwrap();
    assert_eq!(results.price, Some((eur(900), eur(3200))));
}

#[test]
fn availability() {
    let mut q = query();
    q.available = Some(false);
    q.group = Group::Variant;
    assert_eq!(hits(&run(&q)), [v(41), v(12)]);
}

#[test]
fn pages_by_offset() {
    let mut q = query();
    q.offset = 1;
    q.limit = 2;
    let results = run(&q);
    assert_eq!((results.total, hits(&results)), (5, vec![v(31), v(21)]));
}

#[test]
fn markets_are_apart() {
    let mut q = query();
    q.market = MarketId::from_uuid(Uuid::from_u128(2));
    let results = run(&q);
    assert_eq!(hits(&results), [v(31)]);
    assert_eq!(results.hits[0].document.price.amount(), 1100);
}
