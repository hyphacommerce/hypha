//! What every search engine, queue and settings store must do (design note 0019).
//!
//! An engine is held to `hypha_search_index::reference::search`: the fixture's documents are
//! loaded, and every query in a battery (terms, filters, price, availability, groups,
//! sorts, pages, facets) must answer exactly as the reference does, ranking included.

use hypha_core::{MarketId, ProductId, RepoError, Timestamp, VariantId, Version, Versioned};
use hypha_search_index::{
    FacetKey, FacetSetting, Filter, Group, PriceBounds, SearchDocument, SearchIndex, SearchQuery,
    SearchQueue, SearchSettings, SearchSettingsRepo, Sort, Target, keys, reference, terms,
};
use uuid::Uuid;

use super::scenario;
use crate::search_fixture;

fn market(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(n))
}

fn product(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

/// Loads the fixture into `index`, product by product per market.
async fn load<I: SearchIndex>(index: &I, documents: &[SearchDocument]) {
    let mut keys: Vec<(MarketId, ProductId)> =
        documents.iter().map(|d| (d.market, d.product)).collect();
    keys.sort();
    keys.dedup();
    for (m, p) in keys {
        let docs: Vec<SearchDocument> = documents
            .iter()
            .filter(|d| d.market == m && d.product == p)
            .cloned()
            .collect();
        index.replace(m, p, &docs).await.unwrap();
    }
}

fn query(market_n: u128) -> SearchQuery {
    SearchQuery {
        market: market(market_n),
        terms: vec![],
        filters: vec![],
        price: PriceBounds::default(),
        available: None,
        sort: Sort::Relevance,
        group: Group::Listing,
        facets: vec![
            FacetKey::ProductType,
            FacetKey::Vendor,
            FacetKey::Option("color".to_string()),
            FacetKey::Option("size".to_string()),
            FacetKey::Tag,
        ],
        offset: 0,
        limit: 10,
    }
}

fn filter(key: FacetKey, values: &[&str]) -> Filter {
    Filter {
        key,
        values: values.iter().map(|v| v.to_string()).collect(),
    }
}

/// Runs every `SearchIndex` scenario, each on a fresh engine from `make`.
pub async fn search_index<I: SearchIndex>(make: impl AsyncFn() -> I) {
    scenario("search index: replace adds, replaces and removes a product's documents");
    replace(&make().await).await;
    scenario("search index: every query answers as the reference does");
    battery(&make().await).await;
    scenario("search index: option names, most used first");
    option_names(&make().await).await;
}

async fn replace<I: SearchIndex>(index: &I) {
    let fixture = search_fixture();
    assert_eq!(index.count().await, Ok(0));
    load(index, &fixture).await;
    assert_eq!(index.count().await, Ok(fixture.len() as u64));

    // The shirt loses a variant and changes price; the other market is untouched.
    let mut shirt: Vec<SearchDocument> = fixture
        .iter()
        .filter(|d| d.product == product(1))
        .cloned()
        .collect();
    shirt.pop();
    shirt[0].price = hypha_core::Money::new(1, shirt[0].price.currency()).unwrap();
    index.replace(market(1), product(1), &shirt).await.unwrap();
    let mut all = query(1);
    all.group = Group::Variant;
    all.limit = 100;
    let mut expected: Vec<SearchDocument> = fixture
        .iter()
        .filter(|d| d.product != product(1))
        .cloned()
        .collect();
    expected.extend(shirt.clone());
    assert_eq!(
        index.search(&all).await.unwrap(),
        reference::search(&expected, &all)
    );

    // Empty removes the product from that market only.
    index.replace(market(1), product(3), &[]).await.unwrap();
    expected.retain(|d| !(d.market == market(1) && d.product == product(3)));
    assert_eq!(
        index.search(&all).await.unwrap(),
        reference::search(&expected, &all)
    );
    let mut other = all.clone();
    other.market = market(2);
    assert_eq!(index.search(&other).await.unwrap().total, 1);
    assert_eq!(index.count().await, Ok(expected.len() as u64));
}

async fn battery<I: SearchIndex>(index: &I) {
    let fixture = search_fixture();
    load(index, &fixture).await;

    let terms_ = |q: &str| terms::terms(q);
    let mut bases: Vec<SearchQuery> = Vec::new();
    let mut base = |f: &dyn Fn(&mut SearchQuery)| {
        let mut q = query(1);
        f(&mut q);
        bases.push(q);
    };
    base(&|_| {});
    base(&|q| q.terms = terms_("shirt"));
    base(&|q| q.terms = terms_("lin"));
    base(&|q| q.terms = terms_("linen shirt"));
    base(&|q| q.terms = terms_("ČAŠ"));
    base(&|q| q.terms = terms_("ls-red"));
    base(&|q| q.terms = terms_("tea"));
    base(&|q| q.terms = terms_("zzz"));
    base(&|q| q.filters = vec![filter(FacetKey::Option("color".into()), &["Red"])]);
    base(&|q| q.filters = vec![filter(FacetKey::Option("color".into()), &["Red", "Blue"])]);
    base(&|q| {
        q.filters = vec![
            filter(FacetKey::Vendor, &["Acme"]),
            filter(FacetKey::Option("size".into()), &["S"]),
        ]
    });
    base(&|q| q.filters = vec![filter(FacetKey::Tag, &["Summer"])]);
    base(&|q| q.filters = vec![filter(FacetKey::ProductType, &["Nothing"])]);
    base(&|q| {
        q.price = PriceBounds {
            min: Some(1500),
            max: Some(3000),
        }
    });
    base(&|q| q.price.min = Some(3001));
    base(&|q| q.available = Some(true));
    base(&|q| q.available = Some(false));
    base(&|q| {
        q.terms = terms_("shirt");
        q.filters = vec![filter(FacetKey::Option("color".into()), &["Red"])];
        q.available = Some(true);
    });
    base(&|q| {
        q.offset = 1;
        q.limit = 2;
    });
    base(&|q| {
        q.offset = 9;
        q.limit = 5;
    });
    base(&|q| q.market = market(2));
    base(&|q| q.market = market(9));

    for base in bases {
        for group in [Group::Listing, Group::Product, Group::Variant] {
            for sort in [
                Sort::Relevance,
                Sort::Newest,
                Sort::PriceAsc,
                Sort::PriceDesc,
                Sort::Title,
            ] {
                let mut q = base.clone();
                q.group = group;
                q.sort = sort;
                assert_eq!(
                    index.search(&q).await.unwrap(),
                    reference::search(&fixture, &q),
                    "{q:?}"
                );
            }
        }
    }
}

async fn option_names<I: SearchIndex>(index: &I) {
    load(index, &search_fixture()).await;
    // Five documents have a colour and a size each; names are lowercased.
    assert_eq!(
        index.option_names(market(1)).await,
        Ok(vec!["color".to_string(), "size".to_string()])
    );
    assert_eq!(index.option_names(market(2)).await, Ok(vec![]));
}

// ---------- the queue ----------

fn at(secs: i64) -> Timestamp {
    Timestamp::from_unix_micros(secs * 1_000_000)
}

fn variant(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

/// Runs every `SearchQueue` scenario, each on a fresh queue from `make`.
pub async fn search_queue<Q: SearchQueue>(make: impl AsyncFn() -> Q) {
    scenario("search queue: a target is queued once, oldest leased first");
    queued_once(&make().await).await;
    scenario("search queue: a leased mark waits for its lease to end");
    leases(&make().await).await;
    scenario("search queue: finishing keeps a mark marked again meanwhile");
    marked_again(&make().await).await;
}

async fn queued_once<Q: SearchQueue>(queue: &Q) {
    let targets = [
        Target::Product(product(2)),
        Target::Variant(variant(7)),
        Target::All,
    ];
    queue.mark(&targets).await.unwrap();
    queue
        .mark(&[Target::Product(product(2)), Target::Product(product(1))])
        .await
        .unwrap();
    assert_eq!(queue.queued().await, Ok(4));
    let leased = queue.lease(at(0), at(60), 3).await.unwrap();
    assert_eq!(
        leased.iter().map(|m| m.target).collect::<Vec<_>>(),
        [targets[0], targets[1], targets[2]],
        "oldest marked first; marking again doesn't move it back"
    );
    queue.finish(&leased).await.unwrap();
    assert_eq!(queue.queued().await, Ok(1));
    let rest = queue.lease(at(0), at(60), 10).await.unwrap();
    assert_eq!(
        rest.iter().map(|m| m.target).collect::<Vec<_>>(),
        [Target::Product(product(1))]
    );
}

async fn leases<Q: SearchQueue>(queue: &Q) {
    queue.mark(&[Target::All]).await.unwrap();
    let first = queue.lease(at(0), at(60), 10).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(queue.lease(at(59), at(120), 10).await, Ok(vec![]));
    // The lease ended: another runner takes it (the first died).
    let again = queue.lease(at(60), at(120), 10).await.unwrap();
    assert_eq!(again, first);
    queue.finish(&again).await.unwrap();
    assert_eq!(queue.queued().await, Ok(0));
    // Finishing what's gone does nothing.
    queue.finish(&first).await.unwrap();
}

async fn marked_again<Q: SearchQueue>(queue: &Q) {
    let target = Target::Product(product(1));
    queue.mark(&[target]).await.unwrap();
    let leased = queue.lease(at(0), at(60), 10).await.unwrap();
    queue.mark(&[target]).await.unwrap();
    queue.finish(&leased).await.unwrap();
    assert_eq!(queue.queued().await, Ok(1), "the newer mark stays");
    // And its lease is free at once.
    let next = queue.lease(at(1), at(61), 10).await.unwrap();
    assert_eq!(next.len(), 1);
    assert!(next[0].generation > leased[0].generation);
    queue.finish(&next).await.unwrap();
    assert_eq!(queue.queued().await, Ok(0));
}

// ---------- settings ----------

/// Runs every `SearchSettingsRepo` scenario, each on a fresh repo from `make`.
pub async fn search_settings_repo<R: SearchSettingsRepo>(make: impl AsyncFn() -> R) {
    scenario("search settings: none until stored; every field round trips");
    settings_round_trip(&make().await).await;
    scenario("search settings: one record, updated from its version only");
    settings_versions(&make().await).await;
}

fn configured() -> SearchSettings {
    let mut s = SearchSettings::default();
    s.set_split_by(vec!["Color".to_string(), "Material".to_string()])
        .unwrap();
    s.set_facets(vec![
        FacetSetting {
            key: FacetKey::Option("size".to_string()),
            label: Some("Size".to_string()),
        },
        FacetSetting {
            key: FacetKey::Vendor,
            label: None,
        },
    ])
    .unwrap();
    s
}

async fn settings_round_trip<R: SearchSettingsRepo>(repo: &R) {
    assert_eq!(repo.get().await, Ok(None));
    let mut s = configured();
    repo.insert(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(1));
    let stored = repo.get().await.unwrap().unwrap();
    assert_eq!(stored, s);
    assert!(hypha_core::Stamped::stamps(&stored).is_some());
}

async fn settings_versions<R: SearchSettingsRepo>(repo: &R) {
    let mut s = configured();
    repo.insert(&mut s).await.unwrap();
    let mut again = SearchSettings::default();
    assert_eq!(
        repo.insert(&mut again).await,
        Err(RepoError::Duplicate(keys::SETTINGS))
    );

    let mut stale = repo.get().await.unwrap().unwrap();
    s.set_split_by(vec![]).unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(2));
    assert_eq!(repo.get().await, Ok(Some(s.clone())));
    stale.set_facets(vec![]).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
}
