use std::collections::{HashMap, HashSet};

use hypha_core::{
    Actor, AuthError, CatalogId, CountryCode, CountryError, Ctx, Currency, MarketId, Money,
    MoneyError, Price, PriceListId, ProductId, Scope, Timestamp, UserId, VariantId,
};
use hypha_markets::{
    CatalogContext, CatalogRepo, CommandError, CompareAtMode, CreateCatalog, CreateMarket,
    CreatePriceList, EntryProblem, FixedPrice, FixedPriceProblem, Market, MarketError, MarketRepo,
    PriceListError, PriceListKind, PriceListRepo, PriceListStatus, SetFixedPrice, UpdateMarket,
    UpdatePriceList, add_catalog_products, add_market_country, create_catalog, create_market,
    create_price_list, remove_catalog_products, remove_fixed_prices, remove_market_country,
    set_fixed_prices, update_market, update_price_list,
};
use hypha_testkit::{
    InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPorts, InMemoryPriceListRepo, SequentialIds,
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
        currency: Currency::RSD,
        prices_include_tax: true,
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
    assert!(market.prices_include_tax());
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
    named_list("Euro prices", Some(adjustment))
}

fn named_list(name: &str, adjustment: Option<i32>) -> CreatePriceList {
    CreatePriceList {
        name: name.to_string(),
        description: None,
        currency: Currency::EUR,
        kind: PriceListKind::Override,
        status: PriceListStatus::Active,
        starts_at: None,
        ends_at: None,
        compare_at: CompareAtMode::Adjusted,
        adjustment,
    }
}

fn fixed(variant: u128, amount: i64, currency: Currency) -> SetFixedPrice {
    SetFixedPrice {
        variant: vid(variant),
        price: Money::new(amount, currency).unwrap(),
        compare_at: None,
    }
}

fn eur(amount: i64) -> FixedPrice {
    FixedPrice::plain(Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap())
}

/// A stored EUR price list with no adjustment.
fn stored_list(lists: &InMemoryPriceListRepo, ids: &SequentialIds) -> PriceListId {
    block_on(create_price_list(&Ctx::system(), lists, ids, eur_list(0)))
        .unwrap()
        .id()
}

fn problem(index: usize, variant: u128, problem: FixedPriceProblem) -> EntryProblem {
    EntryProblem {
        index,
        variant: Some(vid(variant)),
        problem,
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
        eur_list(-1000),
    ))
    .unwrap();

    assert_eq!(list.adjustment().map(|a| a.percent().value()), Some(-1000));
    assert_eq!(list.name(), "Euro prices");
    assert_eq!(block_on(lists.get(list.id())), Ok(Some(list)));
}

#[test]
fn create_price_list_without_adjustment_has_none() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = block_on(create_price_list(
        &Ctx::system(),
        &lists,
        &ids,
        named_list("Sale", None),
    ))
    .unwrap();
    assert_eq!(list.adjustment(), None);
}

#[test]
fn create_price_list_rejects_a_window_ending_before_it_starts() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let at = Timestamp::from_unix_micros;
    let input = CreatePriceList {
        starts_at: Some(at(10)),
        ends_at: Some(at(10)),
        ..named_list("Sale", None)
    };
    assert_eq!(
        block_on(create_price_list(&Ctx::system(), &lists, &ids, input)).map(|l| l.id()),
        Err(CommandError::PriceList(PriceListError::EndsNotAfterStart))
    );
}

#[test]
fn create_price_list_rejects_a_name_taken_ignoring_case() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let first = block_on(create_price_list(
        &Ctx::system(),
        &lists,
        &ids,
        named_list("Wholesale", None),
    ))
    .unwrap();
    assert_eq!(
        block_on(create_price_list(
            &Ctx::system(),
            &lists,
            &ids,
            named_list(" WHOLESALE ", None)
        ))
        .map(|l| l.id()),
        Err(CommandError::PriceListNameTaken {
            name: "Wholesale".to_string(),
            price_list: first.id(),
        })
    );
}

#[test]
fn a_name_taken_by_a_concurrent_insert_names_the_owner() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let first = block_on(create_price_list(
        &Ctx::system(),
        &lists,
        &ids,
        named_list("Wholesale", None),
    ))
    .unwrap();
    // The pre-check misses it, as if the other insert came right after.
    lists.hide_next_lookups(1);
    assert_eq!(
        block_on(create_price_list(
            &Ctx::system(),
            &lists,
            &ids,
            named_list("wholesale", None)
        ))
        .map(|l| l.id()),
        Err(CommandError::PriceListNameTaken {
            name: "Wholesale".to_string(),
            price_list: first.id(),
        })
    );
}

#[test]
fn update_price_list_changes_what_it_is_given() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);
    let at = Timestamp::from_unix_micros;

    let updated = block_on(update_price_list(
        &Ctx::system(),
        &lists,
        list,
        UpdatePriceList {
            name: Some("Black Friday".to_string()),
            kind: Some(PriceListKind::Sale),
            starts_at: Some(Some(at(5))),
            ends_at: Some(Some(at(9))),
            adjustment: Some(None),
            ..UpdatePriceList::default()
        },
    ))
    .unwrap();

    assert_eq!(updated.name(), "Black Friday");
    assert_eq!(updated.kind(), PriceListKind::Sale);
    assert_eq!(
        (updated.window().starts_at(), updated.window().ends_at()),
        (Some(at(5)), Some(at(9)))
    );
    assert_eq!(updated.adjustment(), None);
    assert_eq!(block_on(lists.get(list)), Ok(Some(updated)));
}

#[test]
fn update_price_list_checks_the_window_as_it_ends_up() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let list = stored_list(&lists, &ids);
    let at = Timestamp::from_unix_micros;
    block_on(update_price_list(
        &Ctx::system(),
        &lists,
        list,
        UpdatePriceList {
            ends_at: Some(Some(at(9))),
            ..UpdatePriceList::default()
        },
    ))
    .unwrap();
    assert_eq!(
        block_on(update_price_list(
            &Ctx::system(),
            &lists,
            list,
            UpdatePriceList {
                starts_at: Some(Some(at(9))),
                ..UpdatePriceList::default()
            },
        ))
        .map(|l| l.id()),
        Err(CommandError::PriceList(PriceListError::EndsNotAfterStart))
    );
}

#[test]
fn update_price_list_rejects_another_lists_name() {
    let lists = InMemoryPriceListRepo::default();
    let ids = SequentialIds::default();
    let taken = stored_list(&lists, &ids);
    let other = block_on(create_price_list(
        &Ctx::system(),
        &lists,
        &ids,
        named_list("Retail", None),
    ))
    .unwrap();
    assert_eq!(
        block_on(update_price_list(
            &Ctx::system(),
            &lists,
            other.id(),
            UpdatePriceList {
                name: Some("euro PRICES".to_string()),
                ..UpdatePriceList::default()
            },
        ))
        .map(|l| l.id()),
        Err(CommandError::PriceListNameTaken {
            name: "Euro prices".to_string(),
            price_list: taken,
        })
    );
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
    let ports = InMemoryPorts::default();
    let lists = &ports.price_lists;
    let ids = SequentialIds::default();
    let list = stored_list(lists, &ids);

    block_on(set_fixed_prices(
        &Ctx::system(),
        &ports,
        list,
        vec![fixed(1, 900, Currency::EUR), fixed(2, 0, Currency::EUR)],
    ))
    .unwrap();

    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1), vid(2), vid(3)])),
        Ok(HashMap::from([(vid(1), eur(900)), (vid(2), eur(0))]))
    );
}

#[test]
fn set_fixed_prices_keeps_a_compare_at_price() {
    let ports = InMemoryPorts::default();
    let lists = &ports.price_lists;
    let ids = SequentialIds::default();
    let list = stored_list(lists, &ids);
    let mut entry = fixed(1, 900, Currency::EUR);
    entry.compare_at = Some(Money::new(1200, Currency::EUR).unwrap());

    block_on(set_fixed_prices(&Ctx::system(), &ports, list, vec![entry])).unwrap();

    let price = |a| Price::new(Money::new(a, Currency::EUR).unwrap()).unwrap();
    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1)])),
        Ok(HashMap::from([(
            vid(1),
            FixedPrice::new(price(900), Some(price(1200))).unwrap()
        )]))
    );
}

#[test]
fn set_fixed_prices_reports_every_entrys_problem_and_stores_nothing() {
    let ports = InMemoryPorts::default();
    let lists = &ports.price_lists;
    let ids = SequentialIds::default();
    let list = stored_list(lists, &ids);
    let mut low_compare_at = fixed(4, 900, Currency::EUR);
    low_compare_at.compare_at = Some(Money::new(900, Currency::EUR).unwrap());

    assert_eq!(
        block_on(set_fixed_prices(
            &Ctx::system(),
            &ports,
            list,
            vec![
                fixed(1, 900, Currency::EUR),
                fixed(2, -1, Currency::EUR),
                fixed(3, 900, Currency::USD),
                low_compare_at,
                fixed(1, 800, Currency::EUR),
            ]
        )),
        Err(CommandError::PriceList(PriceListError::InvalidFixedPrices(
            vec![
                problem(1, 2, FixedPriceProblem::Money(MoneyError::Negative)),
                problem(
                    2,
                    3,
                    FixedPriceProblem::CurrencyMismatch {
                        expected: Currency::EUR,
                        found: Currency::USD,
                    }
                ),
                problem(3, 4, FixedPriceProblem::CompareAtNotAbovePrice),
                problem(4, 1, FixedPriceProblem::Duplicate),
            ]
        )))
    );
    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1)])),
        Ok(HashMap::new())
    );
}

#[test]
fn set_fixed_prices_takes_at_most_a_thousand() {
    let ports = InMemoryPorts::default();
    let lists = &ports.price_lists;
    let ids = SequentialIds::default();
    let list = stored_list(lists, &ids);
    let at_most = (1..=1000).map(|n| fixed(n, 100, Currency::EUR)).collect();
    block_on(set_fixed_prices(&Ctx::system(), &ports, list, at_most)).unwrap();

    let too_many = (1..=1001).map(|n| fixed(n, 100, Currency::EUR)).collect();
    assert_eq!(
        block_on(set_fixed_prices(&Ctx::system(), &ports, list, too_many)),
        Err(CommandError::TooManyPrices { max: 1000 })
    );
}

#[test]
fn set_fixed_prices_rejects_unknown_list() {
    let ports = InMemoryPorts::default();
    let unknown = PriceListId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        block_on(set_fixed_prices(&Ctx::system(), &ports, unknown, vec![])),
        Err(CommandError::PriceListNotFound(unknown))
    );
}

#[test]
fn set_fixed_prices_requires_pricing_write() {
    let ports = InMemoryPorts::default();
    let lists = &ports.price_lists;
    let ids = SequentialIds::default();
    let list = stored_list(lists, &ids);

    assert_eq!(
        block_on(set_fixed_prices(
            &user(vec![Scope::PricingRead]),
            &ports,
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
    let ports = InMemoryPorts::default();
    let lists = &ports.price_lists;
    let ids = SequentialIds::default();
    let list = stored_list(lists, &ids);
    block_on(set_fixed_prices(
        &Ctx::system(),
        &ports,
        list,
        vec![fixed(1, 900, Currency::EUR), fixed(2, 800, Currency::EUR)],
    ))
    .unwrap();

    block_on(remove_fixed_prices(
        &Ctx::system(),
        &ports,
        list,
        vec![vid(1)],
    ))
    .unwrap();

    assert_eq!(
        block_on(lists.fixed_prices(list, &[vid(1), vid(2)])),
        Ok(HashMap::from([(vid(2), eur(800))]))
    );
    assert_eq!(
        ports.outbox.types(),
        [
            "price_list.created",
            "price_list.prices_set",
            "price_list.prices_removed"
        ]
    );
}

// ---- update_market ----

#[test]
fn update_market_stores_prices_include_tax() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();
    let market = block_on(create_market(&ctx, &markets, &ids, balkans(&["RS"]))).unwrap();

    let changed = block_on(update_market(
        &ctx,
        &markets,
        market.id(),
        UpdateMarket {
            name: None,
            prices_include_tax: Some(false),
        },
    ))
    .unwrap();

    assert!(!changed.prices_include_tax());
    assert_eq!(
        block_on(markets.get(market.id())),
        Ok(Some(changed.clone()))
    );

    let unchanged = block_on(update_market(
        &ctx,
        &markets,
        market.id(),
        UpdateMarket::default(),
    ))
    .unwrap();
    assert!(
        !unchanged.prices_include_tax(),
        "an absent field keeps its value"
    );
}

#[test]
fn update_market_rejects_unknown_market() {
    let markets = InMemoryMarketRepo::default();
    let unknown = MarketId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(update_market(
            &Ctx::system(),
            &markets,
            unknown,
            UpdateMarket::default()
        )),
        Err(CommandError::MarketNotFound(unknown))
    );
}

// ---- market countries ----

#[test]
fn add_market_country_sells_there_too() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();
    let market = block_on(create_market(&ctx, &markets, &ids, balkans(&["RS"]))).unwrap();

    let changed = block_on(add_market_country(&ctx, &markets, market.id(), "BA")).unwrap();

    assert_eq!(changed.countries(), &[cc("RS"), cc("BA")]);
    assert_eq!(
        block_on(markets.get_by_country(cc("BA"))),
        Ok(Some(changed))
    );
}

#[test]
fn add_market_country_rejects_a_country_another_market_sells_to() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();
    let serbia = block_on(create_market(&ctx, &markets, &ids, balkans(&["RS"]))).unwrap();
    let bosnia = block_on(create_market(&ctx, &markets, &ids, balkans(&["BA"]))).unwrap();

    assert_eq!(
        block_on(add_market_country(&ctx, &markets, bosnia.id(), "RS")),
        Err(CommandError::CountryInOtherMarket {
            country: cc("RS"),
            market: serbia.id(),
        })
    );
    assert_eq!(
        block_on(add_market_country(&ctx, &markets, bosnia.id(), "BA")),
        Err(CommandError::Market(MarketError::DuplicateCountry(cc(
            "BA"
        ))))
    );
    assert_eq!(
        block_on(add_market_country(&ctx, &markets, bosnia.id(), "ba")),
        Err(CommandError::Country(CountryError::NotUppercase))
    );
}

#[test]
fn remove_market_country_keeps_at_least_one() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let ctx = Ctx::system();
    let market = block_on(create_market(&ctx, &markets, &ids, balkans(&["RS", "BA"]))).unwrap();

    let changed = block_on(remove_market_country(&ctx, &markets, market.id(), "BA")).unwrap();
    assert_eq!(changed.countries(), &[cc("RS")]);
    assert_eq!(block_on(markets.get_by_country(cc("BA"))), Ok(None));
    assert_eq!(
        block_on(remove_market_country(&ctx, &markets, market.id(), "RS")),
        Err(CommandError::Market(MarketError::LastCountry))
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
    let ports = InMemoryPorts::default();
    let catalogs = &ports.catalogs;
    let catalog = stored_catalog(catalogs);

    block_on(add_catalog_products(
        &Ctx::system(),
        &ports,
        catalog,
        vec![pid(1), pid(2)],
    ))
    .unwrap();
    block_on(remove_catalog_products(
        &Ctx::system(),
        &ports,
        catalog,
        vec![pid(1)],
    ))
    .unwrap();

    assert_eq!(
        block_on(catalogs.included_products(catalog, &[pid(1), pid(2)])),
        Ok(HashSet::from([pid(2)]))
    );
    let events = ports.outbox.events();
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        types,
        [
            "catalog.changed",
            "catalog.products_added",
            "catalog.products_removed"
        ]
    );
    assert_eq!(
        events[1].data["products"],
        serde_json::json!([pid(1).to_string(), pid(2).to_string()])
    );
}

#[test]
fn add_catalog_products_rejects_unknown_catalog() {
    let ports = InMemoryPorts::default();
    let unknown = CatalogId::from_uuid(Uuid::from_u128(99));

    assert_eq!(
        block_on(add_catalog_products(
            &Ctx::system(),
            &ports,
            unknown,
            vec![pid(1)]
        )),
        Err(CommandError::CatalogNotFound(unknown))
    );
}

#[test]
fn add_catalog_products_requires_markets_write() {
    let ports = InMemoryPorts::default();
    let catalogs = &ports.catalogs;
    let catalog = stored_catalog(catalogs);

    assert_eq!(
        block_on(add_catalog_products(
            &user(vec![Scope::MarketsRead]),
            &ports,
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

#[test]
fn country_taken_by_a_concurrent_insert_names_the_owner() {
    let markets = InMemoryMarketRepo::default();
    let ids = SequentialIds::default();
    let mut first = Market::new(
        MarketId::from_uuid(Uuid::from_u128(500)),
        "First".to_string(),
        vec![cc("RS")],
        Currency::RSD,
    )
    .unwrap();
    block_on(markets.insert(&mut first)).unwrap();

    // The pre-check misses it, as if it was inserted right after.
    markets.hide_next_lookups(1);
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

#[test]
fn market_and_catalog_writes_need_markets_write() {
    let ports = InMemoryPorts::default();
    let ids = SequentialIds::default();
    let market = block_on(create_market(
        &Ctx::system(),
        &ports.markets,
        &ids,
        balkans(&["RS"]),
    ))
    .unwrap()
    .id();
    let catalog = block_on(create_catalog(
        &Ctx::system(),
        &ports.markets,
        &ports.catalogs,
        &ids,
        catalog_for(market),
    ))
    .unwrap()
    .id();
    let reader = user(vec![Scope::MarketsRead]);
    let rename = UpdateMarket {
        name: Some("Serbia".to_string()),
        ..UpdateMarket::default()
    };
    let results = [
        block_on(update_market(&reader, &ports.markets, market, rename)).map(|_| ()),
        block_on(add_market_country(&reader, &ports.markets, market, "BA")).map(|_| ()),
        block_on(remove_market_country(&reader, &ports.markets, market, "RS")).map(|_| ()),
        block_on(create_catalog(
            &reader,
            &ports.markets,
            &ports.catalogs,
            &ids,
            catalog_for(market),
        ))
        .map(|_| ()),
        block_on(remove_catalog_products(
            &reader,
            &ports,
            catalog,
            vec![pid(1)],
        )),
    ];
    for result in results {
        assert_eq!(
            result,
            Err(CommandError::Auth(AuthError::MissingScope(
                Scope::MarketsWrite
            )))
        );
    }
}
