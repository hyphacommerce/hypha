use std::collections::{HashMap, HashSet};

use hypha_core::{
    BasisPoints, CatalogId, CountryCode, Currency, MarketId, Money, Page, PageLimit, PageRequest,
    Price, PriceListId, ProductId, RepoError, Timestamp, VariantId, Version, Versioned,
};
use hypha_markets::{
    Adjustment, Catalog, CatalogContext, CatalogRepo, CompareAtMode, FixedPrice, Market,
    MarketRepo, PriceList, PriceListDraft, PriceListKind, PriceListRepo, PriceListStatus,
    repo::keys,
};
use uuid::Uuid;

use super::scenario;

// ---------- fixtures ----------

fn mid(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(n))
}

fn cid(n: u128) -> CatalogId {
    CatalogId::from_uuid(Uuid::from_u128(n))
}

fn plid(n: u128) -> PriceListId {
    PriceListId::from_uuid(Uuid::from_u128(n))
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn cc(code: &str) -> CountryCode {
    CountryCode::new(code).unwrap()
}

fn market(n: u128, countries: &[&str]) -> Market {
    Market::new(
        mid(n),
        format!("Market {n}"),
        countries.iter().map(|c| cc(c)).collect(),
        Currency::EUR,
    )
    .unwrap()
}

fn catalog(n: u128, market: u128, price_list: Option<PriceListId>) -> Catalog {
    Catalog::new(cid(n), CatalogContext::Market(mid(market)), price_list)
}

fn price_list(n: u128, currency: Currency, bp: i32) -> PriceList {
    PriceList::new(
        plid(n),
        PriceListDraft::new(&format!("List {n}"), currency).adjusted(BasisPoints::new(bp).unwrap()),
    )
    .unwrap()
}

/// A list holding nothing but `name`: no adjustment.
fn named_list(n: u128, name: &str) -> PriceList {
    PriceList::new(plid(n), PriceListDraft::new(name, Currency::EUR)).unwrap()
}

fn fp(amount: i64) -> FixedPrice {
    FixedPrice::plain(eur(amount))
}

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap()
}

// ---------- markets ----------

/// Runs every `MarketRepo` scenario, each on a fresh repo from `make`.
pub async fn market_repo<R: MarketRepo>(make: impl AsyncFn() -> R) {
    scenario("market: insert stores every field at version 1");
    market_round_trips(&make().await).await;
    scenario("market: lookups of unknown markets find nothing");
    market_unknown(&make().await).await;
    scenario("market: update stores changes and bumps the version");
    market_update(&make().await).await;
    scenario("market: a country belongs to one market");
    market_country_key(&make().await).await;
    scenario("market: removing a country frees it");
    market_country_freed(&make().await).await;
    scenario("market: stale and missing updates fail");
    market_stale(&make().await).await;
    scenario("market: list_all returns every market, oldest first");
    market_list_all(&make().await).await;
}

async fn market_list_all<R: MarketRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    for (n, country) in [(3, "HR"), (1, "RS"), (2, "DE")] {
        repo.insert(&mut market(n, &[country])).await.unwrap();
    }
    let ids: Vec<MarketId> = repo
        .list_all()
        .await
        .unwrap()
        .iter()
        .map(|m| m.id())
        .collect();
    assert_eq!(ids, vec![mid(1), mid(2), mid(3)]);
}

async fn market_round_trips<R: MarketRepo>(repo: &R) {
    // Country order is part of the market and must survive.
    let mut m = market(1, &["HR", "DE", "AT"]);
    m.set_prices_include_tax(true);
    repo.insert(&mut m).await.unwrap();
    assert_eq!(m.version(), Version::new(1));
    assert_eq!(repo.get(m.id()).await, Ok(Some(m.clone())));
    assert_eq!(repo.get_by_country(cc("DE")).await, Ok(Some(m.clone())));

    let mut other = market(2, &["RS"]);
    repo.insert(&mut other).await.unwrap();
    let mut all = repo.list_all().await.unwrap();
    all.sort_by_key(|m| m.id().as_uuid());
    assert_eq!(all, vec![m, other]);
}

async fn market_unknown<R: MarketRepo>(repo: &R) {
    assert_eq!(repo.get(mid(99)).await, Ok(None));
    assert_eq!(repo.get_by_country(cc("RS")).await, Ok(None));
    assert_eq!(repo.list_all().await, Ok(vec![]));
}

async fn market_update<R: MarketRepo>(repo: &R) {
    let mut m = market(1, &["HR", "DE"]);
    repo.insert(&mut m).await.unwrap();

    m.add_country(cc("AT")).unwrap();
    m.remove_country(cc("HR")).unwrap();
    m.set_prices_include_tax(true);
    repo.update(&mut m).await.unwrap();

    assert_eq!(m.version(), Version::new(2));
    assert_eq!(repo.get(m.id()).await, Ok(Some(m.clone())));
    assert_eq!(repo.get_by_country(cc("AT")).await, Ok(Some(m)));
    assert_eq!(repo.get_by_country(cc("HR")).await, Ok(None));
}

async fn market_country_key<R: MarketRepo>(repo: &R) {
    repo.insert(&mut market(1, &["RS"])).await.unwrap();

    assert_eq!(
        repo.insert(&mut market(1, &["HR"])).await,
        Err(RepoError::Duplicate(keys::MARKET_ID))
    );
    assert_eq!(
        repo.insert(&mut market(2, &["BA", "RS"])).await,
        Err(RepoError::Duplicate(keys::COUNTRY))
    );
    assert_eq!(
        repo.get(mid(2)).await,
        Ok(None),
        "failed insert stored nothing"
    );

    let mut hr = market(3, &["HR"]);
    repo.insert(&mut hr).await.unwrap();
    let before = hr.clone();
    hr.add_country(cc("RS")).unwrap();
    assert_eq!(
        repo.update(&mut hr).await,
        Err(RepoError::Duplicate(keys::COUNTRY))
    );
    assert_eq!(
        repo.get(mid(3)).await,
        Ok(Some(before)),
        "failed update stored nothing"
    );
}

async fn market_country_freed<R: MarketRepo>(repo: &R) {
    let mut m = market(1, &["RS", "BA"]);
    repo.insert(&mut m).await.unwrap();
    m.remove_country(cc("BA")).unwrap();
    repo.update(&mut m).await.unwrap();

    repo.insert(&mut market(2, &["BA"])).await.unwrap();
}

async fn market_stale<R: MarketRepo>(repo: &R) {
    let mut m = market(1, &["RS"]);
    repo.insert(&mut m).await.unwrap();
    let mut stale = m.clone();
    m.add_country(cc("BA")).unwrap();
    repo.update(&mut m).await.unwrap();

    stale.add_country(cc("ME")).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(m.id()).await, Ok(Some(m)));

    assert_eq!(
        repo.update(&mut market(9, &["HR"])).await,
        Err(RepoError::StaleVersion)
    );
}

// ---------- catalogs ----------

/// Runs every `CatalogRepo` scenario, each on a fresh repo from `make`.
pub async fn catalog_repo<R: CatalogRepo>(make: impl AsyncFn() -> R) {
    scenario("catalog: insert stores every field at version 1");
    catalog_round_trips(&make().await).await;
    scenario("catalog: update stores changes and bumps the version");
    catalog_update(&make().await).await;
    scenario("catalog: unique ID and one catalog per context");
    catalog_keys(&make().await).await;
    scenario("catalog: stale and missing updates fail");
    catalog_stale(&make().await).await;
    scenario("catalog: products are added, removed and queried in batches");
    catalog_products(&make().await).await;
    scenario("catalog: adding products to a missing catalog fails");
    catalog_products_need_the_catalog(&make().await).await;
    scenario("catalog: list_all returns every catalog, oldest first");
    catalog_list_all(&make().await).await;
    scenario("catalog: a catalog's products come in pages, newest first");
    catalog_product_pages(&make().await).await;
}

async fn catalog_list_all<R: CatalogRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    for (n, market) in [(2, 20), (1, 10)] {
        repo.insert(&mut catalog(n, market, None)).await.unwrap();
    }
    let all = repo.list_all().await.unwrap();
    let ids: Vec<CatalogId> = all.iter().map(|c| c.id()).collect();
    assert_eq!(ids, vec![cid(1), cid(2)]);
    assert_eq!(all[0], repo.get(cid(1)).await.unwrap().unwrap());
}

fn page<Id>(after: Option<Id>, limit: u32) -> PageRequest<Id> {
    PageRequest {
        after,
        limit: PageLimit::new(limit).unwrap(),
    }
}

async fn catalog_product_pages<R: CatalogRepo>(repo: &R) {
    let mut a = catalog(1, 10, None);
    let mut b = catalog(2, 20, None);
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();
    assert_eq!(
        repo.product_page(a.id(), page(None, 2)).await,
        Ok(Page {
            items: vec![],
            has_more: false
        })
    );
    repo.add_products(a.id(), &[pid(1), pid(3), pid(2)])
        .await
        .unwrap();
    repo.add_products(b.id(), &[pid(4)]).await.unwrap();

    assert_eq!(
        repo.product_page(a.id(), page(None, 2)).await,
        Ok(Page {
            items: vec![pid(3), pid(2)],
            has_more: true
        })
    );
    assert_eq!(
        repo.product_page(a.id(), page(Some(pid(2)), 2)).await,
        Ok(Page {
            items: vec![pid(1)],
            has_more: false
        })
    );
    // Pages are per catalog, and an unknown catalog has none.
    assert_eq!(
        repo.product_page(b.id(), page(None, 10))
            .await
            .unwrap()
            .items,
        vec![pid(4)]
    );
    assert_eq!(
        repo.product_page(cid(99), page(None, 10))
            .await
            .unwrap()
            .items,
        vec![]
    );
}

async fn catalog_round_trips<R: CatalogRepo>(repo: &R) {
    for mut c in [catalog(1, 10, Some(plid(5))), catalog(2, 20, None)] {
        repo.insert(&mut c).await.unwrap();
        assert_eq!(c.version(), Version::new(1));
        assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));
        assert_eq!(repo.get_by_context(c.context()).await, Ok(Some(c)));
    }
    assert_eq!(repo.get(cid(99)).await, Ok(None));
    assert_eq!(
        repo.get_by_context(CatalogContext::Market(mid(99))).await,
        Ok(None)
    );
}

async fn catalog_update<R: CatalogRepo>(repo: &R) {
    let mut c = catalog(1, 10, None);
    repo.insert(&mut c).await.unwrap();

    c.set_price_list(Some(plid(5)));
    repo.update(&mut c).await.unwrap();
    assert_eq!(c.version(), Version::new(2));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));

    c.set_price_list(None);
    repo.update(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
}

async fn catalog_keys<R: CatalogRepo>(repo: &R) {
    repo.insert(&mut catalog(1, 10, None)).await.unwrap();

    assert_eq!(
        repo.insert(&mut catalog(1, 20, None)).await,
        Err(RepoError::Duplicate(keys::CATALOG_ID))
    );
    assert_eq!(
        repo.insert(&mut catalog(2, 10, None)).await,
        Err(RepoError::Duplicate(keys::CATALOG_CONTEXT))
    );
    assert_eq!(
        repo.get(cid(2)).await,
        Ok(None),
        "failed insert stored nothing"
    );
}

async fn catalog_stale<R: CatalogRepo>(repo: &R) {
    let mut c = catalog(1, 10, None);
    repo.insert(&mut c).await.unwrap();
    let mut stale = c.clone();
    c.set_price_list(Some(plid(5)));
    repo.update(&mut c).await.unwrap();

    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
    assert_eq!(
        repo.update(&mut catalog(9, 90, None)).await,
        Err(RepoError::StaleVersion)
    );
}

async fn catalog_products<R: CatalogRepo>(repo: &R) {
    let mut a = catalog(1, 10, None);
    let mut b = catalog(2, 20, None);
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();

    assert_eq!(
        repo.included_products(a.id(), &[pid(1)]).await,
        Ok(HashSet::new())
    );

    repo.add_products(a.id(), &[pid(1), pid(2), pid(3)])
        .await
        .unwrap();
    // Adding again is not an error.
    repo.add_products(a.id(), &[pid(3), pid(4)]).await.unwrap();
    // Removing a product that isn't there is not an error.
    repo.remove_products(a.id(), &[pid(1), pid(9)])
        .await
        .unwrap();
    repo.add_products(b.id(), &[pid(1)]).await.unwrap();

    assert_eq!(
        repo.included_products(a.id(), &[pid(1), pid(2), pid(3), pid(4), pid(5)])
            .await,
        Ok(HashSet::from([pid(2), pid(3), pid(4)]))
    );
    assert_eq!(
        repo.included_products(a.id(), &[]).await,
        Ok(HashSet::new())
    );
    // Membership is per catalog.
    assert_eq!(
        repo.included_products(b.id(), &[pid(1), pid(2)]).await,
        Ok(HashSet::from([pid(1)]))
    );
    // Membership is not part of the catalog row: updating the catalog keeps it.
    a.set_price_list(Some(plid(5)));
    repo.update(&mut a).await.unwrap();
    assert_eq!(
        repo.included_products(a.id(), &[pid(2)]).await,
        Ok(HashSet::from([pid(2)]))
    );
}

// ---------- price lists ----------

/// Runs every `PriceListRepo` scenario, each on a fresh repo from `make`.
pub async fn price_list_repo<R: PriceListRepo>(make: impl AsyncFn() -> R) {
    scenario("price list: insert stores every field at version 1");
    price_list_round_trips(&make().await).await;
    scenario("price list: update stores changes and bumps the version");
    price_list_update(&make().await).await;
    scenario("price list: unique ID, stale and missing updates");
    price_list_keys_and_stale(&make().await).await;
    scenario("price list: names are unique, ignoring case");
    price_list_names(&make().await).await;
    scenario("price list: a fixed price keeps its compare-at price");
    fixed_price_compare_at(&make().await).await;
    scenario("price list: fixed prices are set, removed and queried in batches");
    price_list_fixed_prices(&make().await).await;
    scenario("price list: fixed prices need the list and each variant once");
    fixed_prices_preconditions(&make().await).await;
    scenario("price list: lists_pricing names the lists with a variant's fixed price");
    lists_pricing(&make().await).await;
    scenario("price list: list_all returns every list, oldest first");
    price_list_list_all(&make().await).await;
    scenario("price list: fixed prices come in pages, newest variant first");
    fixed_price_pages(&make().await).await;
}

async fn price_list_list_all<R: PriceListRepo>(repo: &R) {
    assert_eq!(repo.list_all().await, Ok(vec![]));
    for n in [2, 1] {
        repo.insert(&mut price_list(n, Currency::EUR, -500))
            .await
            .unwrap();
    }
    let all = repo.list_all().await.unwrap();
    let ids: Vec<PriceListId> = all.iter().map(|l| l.id()).collect();
    assert_eq!(ids, vec![plid(1), plid(2)]);
    assert_eq!(all[1], repo.get(plid(2)).await.unwrap().unwrap());
}

async fn fixed_price_pages<R: PriceListRepo>(repo: &R) {
    for n in [1, 2] {
        repo.insert(&mut price_list(n, Currency::EUR, 0))
            .await
            .unwrap();
    }
    repo.set_fixed_prices(
        plid(1),
        &[(vid(1), fp(100)), (vid(3), fp(300)), (vid(2), fp(200))],
    )
    .await
    .unwrap();
    repo.set_fixed_prices(plid(2), &[(vid(4), fp(400))])
        .await
        .unwrap();

    assert_eq!(
        repo.fixed_price_page(plid(1), page(None, 2)).await,
        Ok(Page {
            items: vec![(vid(3), fp(300)), (vid(2), fp(200))],
            has_more: true
        })
    );
    assert_eq!(
        repo.fixed_price_page(plid(1), page(Some(vid(2)), 2)).await,
        Ok(Page {
            items: vec![(vid(1), fp(100))],
            has_more: false
        })
    );
    assert_eq!(
        repo.fixed_price_page(plid(99), page(None, 10))
            .await
            .unwrap()
            .items,
        vec![]
    );
}

async fn lists_pricing<R: PriceListRepo>(repo: &R) {
    for n in [1, 2, 3] {
        repo.insert(&mut price_list(n, Currency::EUR, 0))
            .await
            .unwrap();
    }
    repo.set_fixed_prices(plid(2), &[(vid(1), fp(100))])
        .await
        .unwrap();
    repo.set_fixed_prices(plid(1), &[(vid(1), fp(90)), (vid(2), fp(80))])
        .await
        .unwrap();
    assert_eq!(repo.lists_pricing(vid(1)).await, Ok(vec![plid(1), plid(2)]));
    assert_eq!(repo.lists_pricing(vid(2)).await, Ok(vec![plid(1)]));
    assert_eq!(repo.lists_pricing(vid(9)).await, Ok(vec![]));
}

async fn price_list_round_trips<R: PriceListRepo>(repo: &R) {
    for mut list in [
        price_list(1, Currency::EUR, -1000),
        price_list(2, Currency::RSD, 0),
        price_list(3, Currency::USD, BasisPoints::MIN),
    ] {
        repo.insert(&mut list).await.unwrap();
        assert_eq!(list.version(), Version::new(1));
        assert_eq!(repo.get(list.id()).await, Ok(Some(list)));
    }
    // Every field set, and none.
    let mut full = PriceList::new(
        plid(4),
        PriceListDraft {
            name: "Black Friday".to_string(),
            description: Some("Everything 20% off".to_string()),
            currency: Currency::EUR,
            kind: PriceListKind::Sale,
            status: PriceListStatus::Draft,
            starts_at: Some(Timestamp::from_unix_micros(1_000_000)),
            ends_at: Some(Timestamp::from_unix_micros(2_000_000)),
            compare_at: CompareAtMode::Hidden,
            adjustment: Some(Adjustment::new(BasisPoints::new(-2000).unwrap())),
        },
    )
    .unwrap();
    repo.insert(&mut full).await.unwrap();
    assert_eq!(repo.get(full.id()).await, Ok(Some(full)));
    let mut bare = named_list(5, "Fixed only");
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(repo.get(bare.id()).await, Ok(Some(bare)));
    assert_eq!(repo.get(plid(99)).await, Ok(None));
}

async fn price_list_names<R: PriceListRepo>(repo: &R) {
    repo.insert(&mut named_list(1, "Wholesale")).await.unwrap();
    assert_eq!(
        repo.insert(&mut named_list(2, "WHOLESALE")).await,
        Err(RepoError::Duplicate(keys::PRICE_LIST_NAME))
    );
    let mut other = named_list(2, "Retail");
    repo.insert(&mut other).await.unwrap();
    other.rename("wholesale").unwrap();
    assert_eq!(
        repo.update(&mut other).await,
        Err(RepoError::Duplicate(keys::PRICE_LIST_NAME))
    );
    // Its own name in another case is fine.
    let mut own = repo.get(plid(1)).await.unwrap().unwrap();
    own.rename("WholeSale").unwrap();
    repo.update(&mut own).await.unwrap();
    assert_eq!(
        repo.get(plid(1)).await.unwrap().unwrap().name(),
        "WholeSale"
    );
}

async fn fixed_price_compare_at<R: PriceListRepo>(repo: &R) {
    repo.insert(&mut named_list(1, "Sale")).await.unwrap();
    let was = FixedPrice::new(eur(800), Some(eur(1000))).unwrap();
    repo.set_fixed_prices(plid(1), &[(vid(1), was.clone()), (vid(2), fp(500))])
        .await
        .unwrap();
    assert_eq!(
        repo.fixed_prices(plid(1), &[vid(1), vid(2)]).await,
        Ok(HashMap::from([(vid(1), was.clone()), (vid(2), fp(500))]))
    );
    // Setting it again without one clears it.
    repo.set_fixed_prices(plid(1), &[(vid(1), fp(800))])
        .await
        .unwrap();
    assert_eq!(
        repo.fixed_prices(plid(1), &[vid(1)]).await,
        Ok(HashMap::from([(vid(1), fp(800))]))
    );
    assert_eq!(
        repo.fixed_price_page(plid(1), page(None, 10))
            .await
            .unwrap()
            .items,
        vec![(vid(2), fp(500)), (vid(1), fp(800))]
    );
}

async fn price_list_update<R: PriceListRepo>(repo: &R) {
    let mut list = price_list(1, Currency::EUR, 0);
    repo.insert(&mut list).await.unwrap();

    list.set_adjustment(Some(Adjustment::new(BasisPoints::new(2500).unwrap())));
    repo.update(&mut list).await.unwrap();

    assert_eq!(list.version(), Version::new(2));
    assert_eq!(repo.get(list.id()).await, Ok(Some(list)));
}

async fn price_list_keys_and_stale<R: PriceListRepo>(repo: &R) {
    let mut list = price_list(1, Currency::EUR, 0);
    repo.insert(&mut list).await.unwrap();
    assert_eq!(
        repo.insert(&mut price_list(1, Currency::RSD, 0)).await,
        Err(RepoError::Duplicate(keys::PRICE_LIST_ID))
    );

    let mut stale = list.clone();
    list.set_adjustment(Some(Adjustment::new(BasisPoints::new(-500).unwrap())));
    repo.update(&mut list).await.unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(list.id()).await, Ok(Some(list)));

    assert_eq!(
        repo.update(&mut price_list(9, Currency::EUR, 0)).await,
        Err(RepoError::StaleVersion)
    );
}

async fn price_list_fixed_prices<R: PriceListRepo>(repo: &R) {
    let mut a = price_list(1, Currency::EUR, 0);
    let mut b = price_list(2, Currency::EUR, 0);
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();

    assert_eq!(
        repo.fixed_prices(a.id(), &[vid(1)]).await,
        Ok(HashMap::new())
    );

    repo.set_fixed_prices(
        a.id(),
        &[(vid(1), fp(100)), (vid(2), fp(200)), (vid(3), fp(0))],
    )
    .await
    .unwrap();
    // Setting again replaces.
    repo.set_fixed_prices(a.id(), &[(vid(1), fp(150))])
        .await
        .unwrap();
    // Removing a price that isn't there is not an error.
    repo.remove_fixed_prices(a.id(), &[vid(2), vid(9)])
        .await
        .unwrap();
    repo.set_fixed_prices(b.id(), &[(vid(1), fp(999))])
        .await
        .unwrap();

    assert_eq!(
        repo.fixed_prices(a.id(), &[vid(1), vid(2), vid(3), vid(4)])
            .await,
        Ok(HashMap::from([(vid(1), fp(150)), (vid(3), fp(0))]))
    );
    assert_eq!(repo.fixed_prices(a.id(), &[]).await, Ok(HashMap::new()));
    // Prices are per list.
    assert_eq!(
        repo.fixed_prices(b.id(), &[vid(1)]).await,
        Ok(HashMap::from([(vid(1), fp(999))]))
    );
    // Fixed prices are not part of the list row: updating the list keeps them.
    a.set_adjustment(Some(Adjustment::new(BasisPoints::new(-1000).unwrap())));
    repo.update(&mut a).await.unwrap();
    assert_eq!(
        repo.fixed_prices(a.id(), &[vid(1)]).await,
        Ok(HashMap::from([(vid(1), fp(150))]))
    );
}

/// Caller precondition (the command loads the catalog first): adapters fail rather
/// than store membership of a catalog that doesn't exist.
async fn catalog_products_need_the_catalog<R: CatalogRepo>(repo: &R) {
    assert!(repo.add_products(cid(99), &[pid(1)]).await.is_err());
}

/// Caller preconditions (`PriceList::check_fixed_prices` runs first): the list exists
/// and each variant appears once. Breaking either fails and stores nothing.
async fn fixed_prices_preconditions<R: PriceListRepo>(repo: &R) {
    assert!(
        repo.set_fixed_prices(plid(99), &[(vid(1), fp(100))])
            .await
            .is_err()
    );

    let mut list = price_list(1, Currency::EUR, 0);
    repo.insert(&mut list).await.unwrap();
    assert!(
        repo.set_fixed_prices(list.id(), &[(vid(1), fp(100)), (vid(1), fp(200))])
            .await
            .is_err()
    );
    assert_eq!(
        repo.fixed_prices(list.id(), &[vid(1)]).await,
        Ok(HashMap::new())
    );
}
