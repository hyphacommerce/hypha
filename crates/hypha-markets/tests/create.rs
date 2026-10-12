use hypha_core::{
    Actor, AuthError, CountryCode, CountryError, Ctx, Currency, MarketId, Money, MoneyError,
    ProductId, Scope, UserId, VariantId,
};
use hypha_markets::{
    CatalogContext, CatalogRepo, CommandError, CreateCatalog, CreateMarket, CreatePriceList,
    FixedPrice, MarketError, MarketRepo, MismatchedPrice, PriceListError, PriceListRepo,
    create_catalog, create_market, create_price_list,
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

// ---- create_price_list ----

fn eur_list(adjustment: i32, fixed_prices: Vec<FixedPrice>) -> CreatePriceList {
    CreatePriceList {
        currency: Currency::Eur,
        adjustment,
        fixed_prices,
    }
}

fn fixed(variant: u128, amount: i64, currency: Currency) -> FixedPrice {
    FixedPrice {
        variant: vid(variant),
        price: Money::new(amount, currency),
    }
}

#[test]
fn create_price_list_stores_list() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    let list = block_on(create_price_list(
        &Ctx::system(),
        &lists,
        &ids,
        eur_list(-1000, vec![fixed(1, 900, Currency::Eur)]),
    ))
    .unwrap();

    assert_eq!(list.adjustment().value(), -1000);
    assert_eq!(
        list.fixed_price(vid(1)).map(|p| p.money()),
        Some(Money::new(900, Currency::Eur))
    );
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
            eur_list(-10_001, vec![])
        )),
        Err(CommandError::Money(MoneyError::InvalidBasisPoints(-10_001)))
    );
}

#[test]
fn create_price_list_rejects_negative_fixed_price() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_price_list(
            &Ctx::system(),
            &lists,
            &ids,
            eur_list(0, vec![fixed(1, -1, Currency::Eur)])
        )),
        Err(CommandError::Money(MoneyError::Negative))
    );
}

#[test]
fn create_price_list_rejects_duplicate_fixed_price() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_price_list(
            &Ctx::system(),
            &lists,
            &ids,
            eur_list(
                0,
                vec![fixed(1, 900, Currency::Eur), fixed(1, 800, Currency::Eur)]
            )
        )),
        Err(CommandError::DuplicateFixedPrice(vid(1)))
    );
}

#[test]
fn create_price_list_rejects_fixed_price_in_other_currency() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();

    assert_eq!(
        block_on(create_price_list(
            &Ctx::system(),
            &lists,
            &ids,
            eur_list(0, vec![fixed(1, 900, Currency::Usd)])
        )),
        Err(CommandError::PriceList(PriceListError::CurrencyMismatch {
            expected: Currency::Eur,
            prices: vec![MismatchedPrice {
                variant: vid(1),
                found: Currency::Usd,
            }],
        }))
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
            eur_list(0, vec![])
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PricingWrite
        )))
    );
}

// ---- create_catalog ----

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn catalog_for(market: MarketId) -> CreateCatalog {
    CreateCatalog {
        market,
        products: vec![pid(1), pid(2)],
    }
}

#[test]
fn create_catalog_stores_catalog_for_market() {
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

    assert!(catalog.includes(pid(1)) && catalog.includes(pid(2)));
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
