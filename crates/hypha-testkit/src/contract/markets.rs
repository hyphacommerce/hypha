use std::collections::{HashMap, HashSet};

use hypha_categories::{Category, CategoryRepo, HasCategories, NewCategory};
use hypha_core::{
    BasisPoints, CatalogId, CategoryId, CountryCode, Currency, Handle, Keyset, MarketId, Money,
    Page, PageLimit, PageRequest, Price, PriceListId, ProductId, RepoError, Timestamp, VariantId,
    Version, Versioned,
};
use hypha_markets::{
    Adjustment, AppliesTo, Catalog, CatalogContext, CatalogRepo, CompareAtMode, FixedPrice,
    FixedPriceFilter, HasCatalogs, HasPriceLists, Market, MarketRepo, PriceBreak, PriceList,
    PriceListDraft, PriceListFilter, PriceListKind, PriceListPhase, PriceListRepo, PriceListSort,
    PriceListStatus, repo::keys,
};
use hypha_products::{HasProducts, Product, ProductRepo, VariantPatch};
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

fn catalog(n: u128, market: u128, price_lists: impl IntoIterator<Item = PriceListId>) -> Catalog {
    Catalog::new(cid(n), CatalogContext::Market(mid(market)), price_lists)
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
/// The ports hold price lists too: a catalog's lists must exist.
pub async fn catalog_repo<S: HasCatalogs + HasPriceLists>(make: impl AsyncFn() -> S) {
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
    scenario("catalog: its price lists are a set, replaced by an update");
    catalog_price_lists(&make().await).await;
    scenario("catalog: a price list a catalog holds can't be deleted");
    catalog_holds_its_lists(&make().await).await;
}

/// Stores lists 5 and 6, for catalogs to hold.
async fn seed_lists<S: HasPriceLists>(ports: &S) {
    for n in [5, 6] {
        ports
            .price_lists()
            .insert(&mut price_list(n, Currency::EUR, 0))
            .await
            .unwrap();
    }
}

async fn catalog_price_lists<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
    let mut c = catalog(1, 10, [plid(6), plid(5)]);
    repo.insert(&mut c).await.unwrap();
    assert_eq!(c.price_lists(), [plid(5), plid(6)]);
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));
    c.set_price_lists(vec![plid(6)]).unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(
        repo.get_by_context(c.context())
            .await
            .unwrap()
            .unwrap()
            .price_lists(),
        [plid(6)]
    );
    // A list may sit on several catalogs.
    let mut other = catalog(2, 20, [plid(6)]);
    repo.insert(&mut other).await.unwrap();
    let all = repo.list_all().await.unwrap();
    assert_eq!(all[0].price_lists(), [plid(6)]);
    assert_eq!(all[1].price_lists(), [plid(6)]);
}

async fn catalog_holds_its_lists<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let mut c = catalog(1, 10, [plid(5)]);
    ports.catalogs().insert(&mut c).await.unwrap();
    let mut held = ports.price_lists().get(plid(5)).await.unwrap().unwrap();
    held.delete();
    assert_eq!(
        ports.price_lists().delete(&mut held).await,
        Err(RepoError::Reference(keys::CATALOG_PRICE_LIST))
    );
    assert!(ports.price_lists().get(plid(5)).await.unwrap().is_some());

    c.set_price_lists(vec![]).unwrap();
    ports.catalogs().update(&mut c).await.unwrap();
    let mut free = ports.price_lists().get(plid(5)).await.unwrap().unwrap();
    free.delete();
    ports.price_lists().delete(&mut free).await.unwrap();
    assert_eq!(ports.price_lists().get(plid(5)).await, Ok(None));
}

async fn catalog_list_all<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
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

async fn catalog_product_pages<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
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

async fn catalog_round_trips<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
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

async fn catalog_update<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
    let mut c = catalog(1, 10, None);
    repo.insert(&mut c).await.unwrap();

    c.set_price_lists(vec![plid(5)]).unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(c.version(), Version::new(2));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c.clone())));

    c.set_price_lists(vec![]).unwrap();
    repo.update(&mut c).await.unwrap();
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
}

async fn catalog_keys<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
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

async fn catalog_stale<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
    let mut c = catalog(1, 10, None);
    repo.insert(&mut c).await.unwrap();
    let mut stale = c.clone();
    c.set_price_lists(vec![plid(5)]).unwrap();
    repo.update(&mut c).await.unwrap();

    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(repo.get(c.id()).await, Ok(Some(c)));
    assert_eq!(
        repo.update(&mut catalog(9, 90, None)).await,
        Err(RepoError::StaleVersion)
    );
}

async fn catalog_products<S: HasCatalogs + HasPriceLists>(ports: &S) {
    seed_lists(ports).await;
    let repo = ports.catalogs();
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
    a.set_price_lists(vec![plid(5)]).unwrap();
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
    scenario("price list: a fixed price keeps its quantity prices, replaced with it");
    fixed_price_breaks(&make().await).await;
    scenario("price list: an adjustment keeps what it applies to");
    price_list_applies_to(&make().await).await;
    scenario("price list: get_many returns those that exist, oldest first");
    price_list_get_many(&make().await).await;
    scenario("price list: lists are filtered by kind, phase and ID, and sorted");
    price_list_list(&make().await).await;
    scenario("price list: fixed prices of several lists come in one read, with counts");
    fixed_prices_of_several_lists(&make().await).await;
    scenario("price list: deleting one takes its fixed prices");
    price_list_delete(&make().await).await;
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
        repo.fixed_price_page(plid(1), &FixedPriceFilter::default(), page(None, 2))
            .await,
        Ok(Page {
            items: vec![(vid(3), fp(300)), (vid(2), fp(200))],
            has_more: true
        })
    );
    assert_eq!(
        repo.fixed_price_page(plid(1), &FixedPriceFilter::default(), page(Some(vid(2)), 2))
            .await,
        Ok(Page {
            items: vec![(vid(1), fp(100))],
            has_more: false
        })
    );
    assert_eq!(
        repo.fixed_price_page(plid(99), &FixedPriceFilter::default(), page(None, 10))
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
            adjustment: Some(Adjustment::all(BasisPoints::new(-2000).unwrap())),
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
        repo.fixed_price_page(plid(1), &FixedPriceFilter::default(), page(None, 10))
            .await
            .unwrap()
            .items,
        vec![(vid(2), fp(500)), (vid(1), fp(800))]
    );
}

async fn fixed_price_breaks<R: PriceListRepo>(repo: &R) {
    repo.insert(&mut named_list(1, "Wholesale")).await.unwrap();
    let at = |q, a| PriceBreak {
        min_quantity: q,
        price: eur(a),
    };
    let tiered = FixedPrice::new(eur(1000), Some(eur(1200)))
        .unwrap()
        .with_breaks(vec![at(5, 900), at(10, 800)])
        .unwrap();
    repo.set_fixed_prices(plid(1), &[(vid(1), tiered.clone()), (vid(2), fp(500))])
        .await
        .unwrap();
    assert_eq!(
        repo.fixed_prices(plid(1), &[vid(1), vid(2)]).await,
        Ok(HashMap::from([(vid(1), tiered.clone()), (vid(2), fp(500))]))
    );
    assert_eq!(
        repo.fixed_prices_in(&[plid(1)], &[vid(1)]).await,
        Ok(HashMap::from([(
            plid(1),
            HashMap::from([(vid(1), tiered.clone())])
        )]))
    );
    assert_eq!(
        repo.fixed_price_page(plid(1), &FixedPriceFilter::default(), page(None, 10))
            .await
            .unwrap()
            .items,
        vec![(vid(2), fp(500)), (vid(1), tiered)]
    );
    // Set again with one break: the other goes.
    let fewer = FixedPrice::plain(eur(1000))
        .with_breaks(vec![at(3, 950)])
        .unwrap();
    repo.set_fixed_prices(plid(1), &[(vid(1), fewer.clone())])
        .await
        .unwrap();
    assert_eq!(
        repo.fixed_prices(plid(1), &[vid(1)]).await,
        Ok(HashMap::from([(vid(1), fewer)]))
    );
    // Removing the price takes its breaks: a new one starts without.
    repo.remove_fixed_prices(plid(1), &[vid(1)]).await.unwrap();
    repo.set_fixed_prices(plid(1), &[(vid(1), fp(990))])
        .await
        .unwrap();
    assert_eq!(
        repo.fixed_prices(plid(1), &[vid(1)]).await,
        Ok(HashMap::from([(vid(1), fp(990))]))
    );
}

async fn price_list_applies_to<R: PriceListRepo>(repo: &R) {
    let bp = BasisPoints::new(-2000).unwrap();
    let cat = |n| CategoryId::from_uuid(Uuid::from_u128(n));
    for (n, applies_to) in [
        (1, AppliesTo::Products(vec![pid(2), pid(1)])),
        (2, AppliesTo::Categories(vec![cat(7)])),
        (3, AppliesTo::All),
    ] {
        let mut list = named_list(n, &format!("Scoped {n}"));
        list.set_adjustment(Some(Adjustment::new(bp, applies_to).unwrap()));
        repo.insert(&mut list).await.unwrap();
        assert_eq!(repo.get(list.id()).await, Ok(Some(list)));
    }
}

async fn price_list_get_many<R: PriceListRepo>(repo: &R) {
    for n in [3, 1, 2] {
        repo.insert(&mut price_list(n, Currency::EUR, 0))
            .await
            .unwrap();
    }
    let found = repo.get_many(&[plid(3), plid(9), plid(1)]).await.unwrap();
    let ids: Vec<PriceListId> = found.iter().map(|l| l.id()).collect();
    assert_eq!(ids, [plid(1), plid(3)]);
    assert_eq!(repo.get_many(&[]).await, Ok(vec![]));
}

async fn price_list_list<R: PriceListRepo>(repo: &R) {
    let at = |s: i64| Timestamp::from_unix_micros(s * 1_000_000);
    let mut sale = named_list(1, "b sale");
    sale.set_kind(PriceListKind::Sale);
    let mut draft = named_list(2, "C draft");
    draft.set_status(PriceListStatus::Draft);
    let mut scheduled = named_list(3, "a later");
    scheduled.set_window(hypha_markets::Window::new(Some(at(100)), None).unwrap());
    let mut expired = named_list(4, "D over");
    expired.set_window(hypha_markets::Window::new(None, Some(at(10))).unwrap());
    for mut l in [sale, draft, scheduled, expired] {
        repo.insert(&mut l).await.unwrap();
    }
    let ids = |page: Page<PriceList>| page.items.iter().map(|l| l.id()).collect::<Vec<_>>();
    let keyset = |after: Option<Keyset<PriceListId>>, limit| PageRequest {
        after,
        limit: PageLimit::new(limit).unwrap(),
    };
    let everything = PriceListFilter::everything(at(50));
    let list = async |filter: &PriceListFilter, sort| {
        repo.list(filter, sort, keyset(None, 10)).await.unwrap()
    };
    assert_eq!(
        ids(list(&everything, PriceListSort::Newest).await),
        [plid(4), plid(3), plid(2), plid(1)]
    );
    assert_eq!(
        ids(list(&everything, PriceListSort::NameAsc).await),
        [plid(3), plid(1), plid(2), plid(4)]
    );
    for (phase, expected) in [
        (PriceListPhase::Active, plid(1)),
        (PriceListPhase::Draft, plid(2)),
        (PriceListPhase::Scheduled, plid(3)),
        (PriceListPhase::Expired, plid(4)),
    ] {
        let filter = PriceListFilter {
            phase: Some(phase),
            ..everything.clone()
        };
        assert_eq!(ids(list(&filter, PriceListSort::Newest).await), [expected]);
    }
    let sales = PriceListFilter {
        kind: Some(PriceListKind::Sale),
        ..everything.clone()
    };
    assert_eq!(ids(list(&sales, PriceListSort::Newest).await), [plid(1)]);
    let some = PriceListFilter {
        ids: Some(vec![plid(2), plid(4)]),
        ..everything.clone()
    };
    assert_eq!(
        ids(list(&some, PriceListSort::Oldest).await),
        [plid(2), plid(4)]
    );
    let none = PriceListFilter {
        ids: Some(vec![]),
        ..everything.clone()
    };
    assert_eq!(ids(list(&none, PriceListSort::Oldest).await), []);

    // Pages continue after the last item's key and ID.
    let first = repo
        .list(&everything, PriceListSort::NameDesc, keyset(None, 2))
        .await
        .unwrap();
    assert_eq!(
        (ids(first.clone()), first.has_more),
        (vec![plid(4), plid(2)], true)
    );
    let after = Keyset {
        key: PriceListSort::NameDesc.key(&first.items[1]),
        id: plid(2),
    };
    let rest = repo
        .list(&everything, PriceListSort::NameDesc, keyset(Some(after), 2))
        .await
        .unwrap();
    assert_eq!(
        (ids(rest.clone()), rest.has_more),
        (vec![plid(1), plid(3)], false)
    );
}

async fn fixed_prices_of_several_lists<R: PriceListRepo>(repo: &R) {
    for n in [1, 2, 3] {
        repo.insert(&mut price_list(n, Currency::EUR, 0))
            .await
            .unwrap();
    }
    repo.set_fixed_prices(plid(1), &[(vid(1), fp(100)), (vid(2), fp(200))])
        .await
        .unwrap();
    repo.set_fixed_prices(plid(2), &[(vid(1), fp(90))])
        .await
        .unwrap();
    assert_eq!(
        repo.fixed_prices_in(&[plid(1), plid(2), plid(3)], &[vid(1), vid(9)])
            .await,
        Ok(HashMap::from([
            (plid(1), HashMap::from([(vid(1), fp(100))])),
            (plid(2), HashMap::from([(vid(1), fp(90))])),
        ]))
    );
    assert_eq!(
        repo.fixed_prices_in(&[], &[vid(1)]).await,
        Ok(HashMap::new())
    );
    assert_eq!(
        repo.fixed_price_counts(&[plid(1), plid(2), plid(3)]).await,
        Ok(HashMap::from([(plid(1), 2), (plid(2), 1)]))
    );
}

async fn price_list_delete<R: PriceListRepo>(repo: &R) {
    let mut list = price_list(1, Currency::EUR, 0);
    repo.insert(&mut list).await.unwrap();
    repo.set_fixed_prices(plid(1), &[(vid(1), fp(100))])
        .await
        .unwrap();
    list.delete();
    repo.delete(&mut list).await.unwrap();
    assert_eq!(repo.get(plid(1)).await, Ok(None));
    assert_eq!(
        repo.fixed_prices(plid(1), &[vid(1)]).await,
        Ok(HashMap::new())
    );
    // Its name is free again, and a list gone already is fine.
    repo.insert(&mut price_list(2, Currency::EUR, 0))
        .await
        .unwrap();
    repo.delete(&mut list).await.unwrap();
}

/// Runs the scenarios that read other domains' records, as the adapter stores them: the
/// products and categories fixed prices are filtered by, and the category tree.
pub async fn price_list_joins<S>(make: impl AsyncFn() -> S)
where
    S: HasProducts + HasCategories + HasPriceLists,
{
    scenario("price list: fixed prices filtered by product, category and text");
    fixed_price_filters(&make().await).await;
    scenario("price list: a product's categories come with their ancestors");
    product_categories(&make().await).await;
}

fn product_with_sku(n: u128, title: &str, sku: &str) -> Product {
    let mut p = Product::new_simple(
        pid(n),
        Handle::new(&format!("p-{n}")).unwrap(),
        title.to_string(),
        vid(n * 10),
        eur(1000),
    )
    .unwrap();
    p.update_variant(
        vid(n * 10),
        VariantPatch {
            sku: Some(Some(sku.to_string())),
            ..VariantPatch::default()
        },
    )
    .unwrap();
    p
}

fn category(n: u128, parent: Option<u128>) -> Category {
    let id = |n| CategoryId::from_uuid(Uuid::from_u128(n));
    Category::new(NewCategory {
        id: id(n),
        name: format!("Category {n}"),
        handle: Handle::new(&format!("category-{n}")).unwrap(),
        description: None,
        parent: parent.map(id),
        position: 0,
        visible: true,
    })
    .unwrap()
}

async fn fixed_price_filters<S: HasProducts + HasCategories + HasPriceLists>(ports: &S) {
    for mut p in [
        product_with_sku(1, "Blue mug", "MUG-B"),
        product_with_sku(2, "Cup", "CUP-1"),
        product_with_sku(3, "Plate", "PL-MUG"),
    ] {
        ports.products().insert(&mut p).await.unwrap();
    }
    let mut kitchen = category(50, None);
    ports.categories().insert(&mut kitchen).await.unwrap();
    ports
        .categories()
        .add_products(kitchen.id(), &[pid(2)])
        .await
        .unwrap();
    let repo = ports.price_lists();
    repo.insert(&mut price_list(1, Currency::EUR, 0))
        .await
        .unwrap();
    // A variant no product has, too.
    repo.set_fixed_prices(
        plid(1),
        &[
            (vid(10), fp(1)),
            (vid(20), fp(2)),
            (vid(30), fp(3)),
            (vid(99), fp(9)),
        ],
    )
    .await
    .unwrap();
    let variants = async |filter: FixedPriceFilter| {
        repo.fixed_price_page(plid(1), &filter, page(None, 10))
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|(v, _)| v)
            .collect::<Vec<_>>()
    };
    let filter = |product, category, q: Option<&str>| {
        FixedPriceFilter::new(product, category, q.map(str::to_string)).unwrap()
    };
    assert_eq!(
        variants(filter(None, None, None)).await,
        [vid(99), vid(30), vid(20), vid(10)]
    );
    assert_eq!(variants(filter(Some(pid(1)), None, None)).await, [vid(10)]);
    assert_eq!(
        variants(filter(None, Some(kitchen.id()), None)).await,
        [vid(20)]
    );
    // By title or SKU, ignoring case.
    assert_eq!(
        variants(filter(None, None, Some("mug"))).await,
        [vid(30), vid(10)]
    );
    assert_eq!(variants(filter(Some(pid(2)), None, Some("mug"))).await, []);
}

async fn product_categories<S: HasProducts + HasCategories + HasPriceLists>(ports: &S) {
    let id = |n| CategoryId::from_uuid(Uuid::from_u128(n));
    // 1 Kitchen › 2 Mugs › 3 Espresso; 4 Gifts.
    for mut c in [
        category(1, None),
        category(2, Some(1)),
        category(3, Some(2)),
        category(4, None),
    ] {
        ports.categories().insert(&mut c).await.unwrap();
    }
    ports
        .categories()
        .add_products(id(3), &[pid(1)])
        .await
        .unwrap();
    ports
        .categories()
        .add_products(id(4), &[pid(1), pid(2)])
        .await
        .unwrap();
    assert_eq!(
        ports
            .price_lists()
            .product_categories(&[pid(1), pid(2), pid(9)])
            .await,
        Ok(HashMap::from([
            (pid(1), HashSet::from([id(1), id(2), id(3), id(4)])),
            (pid(2), HashSet::from([id(4)])),
        ]))
    );
    assert_eq!(
        ports.price_lists().product_categories(&[]).await,
        Ok(HashMap::new())
    );
}

async fn price_list_update<R: PriceListRepo>(repo: &R) {
    let mut list = price_list(1, Currency::EUR, 0);
    repo.insert(&mut list).await.unwrap();

    list.set_adjustment(Some(Adjustment::all(BasisPoints::new(2500).unwrap())));
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
    list.set_adjustment(Some(Adjustment::all(BasisPoints::new(-500).unwrap())));
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
    a.set_adjustment(Some(Adjustment::all(BasisPoints::new(-1000).unwrap())));
    repo.update(&mut a).await.unwrap();
    assert_eq!(
        repo.fixed_prices(a.id(), &[vid(1)]).await,
        Ok(HashMap::from([(vid(1), fp(150))]))
    );
}

/// Caller precondition (the command loads the catalog first): adapters fail rather
/// than store membership of a catalog that doesn't exist.
async fn catalog_products_need_the_catalog<S: HasCatalogs>(ports: &S) {
    let repo = ports.catalogs();
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
