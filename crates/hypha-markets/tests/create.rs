use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicUsize, Ordering},
};

use hypha_core::{
    Actor, AuthError, CatalogId, CountryCode, CountryError, Ctx, Currency, MarketId, Money,
    MoneyError, Price, PriceListId, ProductId, RepoError, Scope, UserId, VariantId,
};
use hypha_markets::{
    CatalogContext, CatalogRepo, CommandError, CreateCatalog, CreateMarket, CreatePriceList,
    FixedPrice, Market, MarketError, MarketRepo, MismatchedPrice, PriceListError, PriceListRepo,
    add_catalog_products, create_catalog, create_market, create_price_list,
    remove_catalog_products, remove_fixed_prices, set_fixed_prices,
};
use hypha_testkit::{
    InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo, SequentialIds,
};
use pollster::block_on;
use uuid::Uuid;

fn cc(s: &str) -> CountryCode {
    CountryCode::new(s).unwrap()
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn balkans(countries: &[&str]) -> CreateMarket {
    CreateMarket {
        name: "Balkans".to_string(),
        countries: countries.iter().map(|c| c.to_string()).collect(),
        currency: Currency::Rsd,
    }
}

fn user(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
}

// ---- create_market ----

#[test]
fn create_market_stores_market() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();

    let market = block_on(create_market(
        &Ctx::system(),
        &markets,
        &ids,
        balkans(&["RS", "BA"]),
    ))
    .unwrap();

    assert_eq!(market.countries(), &[cc("RS"), cc("BA")]);
    assert_eq!(block_on(markets.get(market.id())), Ok(Some(market)));
}

#[test]
fn create_market_rejects_invalid_country() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_market(
            &Ctx::system(),
            &markets,
            &ids,
            balkans(&["rs"])
        )),
        Err(CommandError::Country(CountryError::NotUppercase))
    );
}

#[test]
fn create_market_rejects_duplicate_country_in_input() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_market(
            &Ctx::system(),
            &markets,
            &ids,
            balkans(&["RS", "RS"])
        )),
        Err(CommandError::Market(MarketError::DuplicateCountry(cc(
            "RS"
        ))))
    );
}

#[test]
fn create_market_rejects_country_owned_by_another_market() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let first = block_on(create_market(
        &Ctx::system(),
        &markets,
        &ids,
        balkans(&["RS"]),
    ))
    .unwrap();

    assert_eq!(
        block_on(create_market(
            &Ctx::system(),
            &markets,
            &ids,
            balkans(&["BA", "RS"])
        )),
        Err(CommandError::CountryInOtherMarket {
            country: cc("RS"),
            market: first.id(),
        })
    );
    assert_eq!(block_on(markets.list_all()).unwrap().len(), 1);
}

#[test]
fn create_market_requires_markets_write() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_market(
            &user(vec![Scope::MarketsRead]),
            &markets,
            &ids,
            balkans(&["RS"])
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MarketsWrite
        )))
    );
}

// ---- price lists and fixed prices ----

fn eur_list(adjustment: i32) -> CreatePriceList {
    CreatePriceList {
        currency: Currency::Eur,
        adjustment,
    }
}

fn fixed(variant: u128, amount: i64, currency: Currency) -> FixedPrice {
    FixedPrice {
        variant: vid(variant),
        price: Money::new(amount, currency).unwrap(),
    }
}

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
}

/// A stored EUR price list with no adjustment.
fn stored_list(lists: &InMemoryPriceListRepo, ids: &SequentialIds) -> PriceListId {
    block_on(create_price_list(&Ctx::system(), lists, ids, eur_list(0)))
        .unwrap()
        .id()
}

#[test]
fn create_price_list_stores_list() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    let list = block_on(create_price_list(
        &Ctx::system(),
        &lists,
        &ids,
        eur_list(-1000),
    ))
    .unwrap();

    assert_eq!(list.adjustment().value(), -1000);
    assert_eq!(block_on(lists.get(list.id())), Ok(Some(list)));
}

#[test]
fn create_price_list_rejects_adjustment_below_minus_100_percent() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_price_list(
            &Ctx::system(),
            &lists,
            &ids,
            eur_list(-10_001)
        )),
        Err(CommandError::Money(MoneyError::InvalidBasisPoints(-10_001)))
    );
}

#[test]
fn create_price_list_requires_pricing_write() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_price_list(
            &user(vec![Scope::MarketsWrite]),
            &lists,
            &ids,
            eur_list(0)
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PricingWrite
        )))
    );
}

#[test]
fn set_fixed_prices_stores_them() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);

    block_on(set_fixed_prices(
        &Ctx::system(),
        &lists,
        list,
        vec![fixed(1, 900, Currency::Eur), fixed(2, 0, Currency::Eur)],
    ))
    .unwrap();

    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1), vid(2), vid(3)])),
        Ok(HashMap::from([(vid(1), eur(900)), (vid(2), eur(0))]))
    );
}

#[test]
fn set_fixed_prices_rejects_negative_price_and_stores_nothing() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);

    assert_eq!(
        block_on(set_fixed_prices(
            &Ctx::system(),
            &lists,
            list,
            vec![fixed(1, 900, Currency::Eur), fixed(2, -1, Currency::Eur)]
        )),
        Err(CommandError::Money(MoneyError::Negative))
    );
    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1)])),
        Ok(HashMap::new())
    );
}

#[test]
fn set_fixed_prices_rejects_duplicate_variant() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);

    assert_eq!(
        block_on(set_fixed_prices(
            &Ctx::system(),
            &lists,
            list,
            vec![fixed(1, 900, Currency::Eur), fixed(1, 800, Currency::Eur)]
        )),
        Err(CommandError::PriceList(
            PriceListError::DuplicateFixedPrice(vid(1))
        ))
    );
}

#[test]
fn set_fixed_prices_rejects_other_currency_and_stores_nothing() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);

    assert_eq!(
        block_on(set_fixed_prices(
            &Ctx::system(),
            &lists,
            list,
            vec![fixed(1, 900, Currency::Eur), fixed(2, 900, Currency::Usd)]
        )),
        Err(CommandError::PriceList(PriceListError::CurrencyMismatch {
            expected: Currency::Eur,
            prices: vec![MismatchedPrice {
                variant: vid(2),
                found: Currency::Usd,
            }],
        }))
    );
    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1)])),
        Ok(HashMap::new())
    );
}

#[test]
fn set_fixed_prices_rejects_unknown_list() {
    let lists = InMemoryPriceListRepo::default();
    let unknown = PriceListId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        block_on(set_fixed_prices(&Ctx::system(), &lists, unknown, vec![])),
        Err(CommandError::PriceListNotFound(unknown))
    );
}

#[test]
fn set_fixed_prices_requires_pricing_write() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);

    assert_eq!(
        block_on(set_fixed_prices(
            &user(vec![Scope::PricingRead]),
            &lists,
            list,
            vec![]
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PricingWrite
        )))
    );
}

#[test]
fn remove_fixed_prices_deletes_them() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);
    block_on(set_fixed_prices(
        &Ctx::system(),
        &lists,
        list,
        vec![fixed(1, 900, Currency::Eur), fixed(2, 800, Currency::Eur)],
    ))
    .unwrap();

    block_on(remove_fixed_prices(
        &Ctx::system(),
        &lists,
        list,
        vec![vid(1)],
    ))
    .unwrap();

    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1), vid(2)])),
        Ok(HashMap::from([(vid(2), eur(800))]))
    );
}

// ---- catalogs and their products ----

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn catalog_for(market: MarketId) -> CreateCatalog {
    CreateCatalog { market }
}

#[test]
fn create_catalog_stores_empty_catalog_for_market() {
    let markets = InMemoryMarketRepo::default();
    let catalogs = InMemoryCatalogRepo::default();
    let ids = SequentialIds::default();
    let market = block_on(create_market(
        &Ctx::system(),
        &markets,
        &ids,
        balkans(&["RS"]),
    ))
    .unwrap();

    let catalog = block_on(create_catalog(
        &Ctx::system(),
        &markets,
        &catalogs,
        &ids,
        catalog_for(market.id()),
    ))
    .unwrap();

    assert_eq!(catalog.price_list(), None);
    assert_eq!(
        block_on(catalogs.get_by_context(CatalogContext::Market(market.id()))),
        Ok(Some(catalog))
    );
}

#[test]
fn create_catalog_rejects_unknown_market() {
    let markets = InMemoryMarketRepo::default();
    let catalogs = InMemoryCatalogRepo::default();
    let ids = SequentialIds::default();
    let unknown = MarketId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        block_on(create_catalog(
            &Ctx::system(),
            &markets,
            &catalogs,
            &ids,
            catalog_for(unknown)
        )),
        Err(CommandError::MarketNotFound(unknown))
    );
}

#[test]
fn create_catalog_rejects_second_catalog_for_market() {
    let markets = InMemoryMarketRepo::default();
    let catalogs = InMemoryCatalogRepo::default();
    let ids = SequentialIds::default();
    let market = block_on(create_market(
        &Ctx::system(),
        &markets,
        &ids,
        balkans(&["RS"]),
    ))
    .unwrap();
    let first = block_on(create_catalog(
        &Ctx::system(),
        &markets,
        &catalogs,
        &ids,
        catalog_for(market.id()),
    ))
    .unwrap();

    assert_eq!(
        block_on(create_catalog(
            &Ctx::system(),
            &markets,
            &catalogs,
            &ids,
            catalog_for(market.id())
        )),
        Err(CommandError::CatalogExists {
            market: market.id(),
            catalog: first.id(),
        })
    );
}

/// A stored catalog for a fresh RS market.
fn stored_catalog(catalogs: &InMemoryCatalogRepo) -> CatalogId {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let market = block_on(create_market(
        &Ctx::system(),
        &markets,
        &ids,
        balkans(&["RS"]),
    ))
    .unwrap();
    block_on(create_catalog(
        &Ctx::system(),
        &markets,
        catalogs,
        &ids,
        catalog_for(market.id()),
    ))
    .unwrap()
    .id()
}

#[test]
fn add_and_remove_catalog_products() {
    let catalogs = InMemoryCatalogRepo::default();
    let catalog = stored_catalog(&catalogs);

    block_on(add_catalog_products(
        &Ctx::system(),
        &catalogs,
        catalog,
        vec![pid(1), pid(2)],
    ))
    .unwrap();
    block_on(remove_catalog_products(
        &Ctx::system(),
        &catalogs,
        catalog,
        vec![pid(1)],
    ))
    .unwrap();

    assert_eq!(
        block_on(catalogs.included_products(catalog, &[pid(1), pid(2)])),
        Ok(HashSet::from([pid(2)]))
    );
}

#[test]
fn add_catalog_products_rejects_unknown_catalog() {
    let catalogs = InMemoryCatalogRepo::default();
    let unknown = CatalogId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        block_on(add_catalog_products(
            &Ctx::system(),
            &catalogs,
            unknown,
            vec![pid(1)]
        )),
        Err(CommandError::CatalogNotFound(unknown))
    );
}

#[test]
fn add_catalog_products_requires_markets_write() {
    let catalogs = InMemoryCatalogRepo::default();
    let catalog = stored_catalog(&catalogs);

    assert_eq!(
        block_on(add_catalog_products(
            &user(vec![Scope::MarketsRead]),
            &catalogs,
            catalog,
            vec![pid(1)]
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MarketsWrite
        )))
    );
    assert_eq!(
        block_on(catalogs.included_products(catalog, &[pid(1)])),
        Ok(HashSet::new())
    );
}

// ---- races: pre-check passes, the repo's unique key catches it ----

/// Hides existing markets from the first `list_all` (the command's pre-check), as if
/// another request inserted one right after it.
#[derive(Default)]
struct RacingMarkets {
    inner: InMemoryMarketRepo,
    lists: AtomicUsize,
}

impl MarketRepo for RacingMarkets {
    async fn get(&self, id: MarketId) -> Result<Option<Market>, RepoError> {
        self.inner.get(id).await
    }
    async fn list_all(&self) -> Result<Vec<Market>, RepoError> {
        if self.lists.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(vec![]);
        }
        self.inner.list_all().await
    }
    async fn insert(&self, market: &mut Market) -> Result<(), RepoError> {
        self.inner.insert(market).await
    }
    async fn update(&self, market: &mut Market) -> Result<(), RepoError> {
        self.inner.update(market).await
    }
}

#[test]
fn country_taken_by_a_concurrent_insert_names_the_owner() {
    let markets = RacingMarkets::default();
    let ids = SequentialIds::default();
    let mut first = Market::new(
        MarketId::from_uuid(Uuid::from_u128(500)),
        "First".to_string(),
        vec![cc("RS")],
        Currency::Rsd,
    )
    .unwrap();
    block_on(markets.inner.insert(&mut first)).unwrap();

    assert_eq!(
        block_on(create_market(
            &Ctx::system(),
            &markets,
            &ids,
            balkans(&["BA", "RS"])
        )),
        Err(CommandError::CountryInOtherMarket {
            country: cc("RS"),
            market: first.id(),
        })
    );
}
