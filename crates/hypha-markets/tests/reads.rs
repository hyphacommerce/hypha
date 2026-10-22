//! The admin's reads of its setup.

use hypha_core::{
    Actor, AuthError, BasisPoints, CatalogId, CountryCode, Ctx, Currency, MarketId, Money, Page,
    PageLimit, PageRequest, Price, PriceListId, ProductId, Scope, UserId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, CommandError, Market, MarketRepo, MarketView, PriceList,
    PriceListRepo, get_catalog, get_market, list_catalog_products, list_fixed_prices, list_markets,
    list_markets_only, list_price_lists,
};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists};
use hypha_testkit::InMemoryPorts;
use pollster::block_on;
use uuid::Uuid;

fn ctx(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
}

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

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap()
}

fn first(limit: u32) -> PageRequest<ProductId> {
    PageRequest {
        after: None,
        limit: PageLimit::new(limit).unwrap(),
    }
}

/// Two markets (1: HR with catalog 10, 2: RS without one) and price list 20.
fn world() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    for (n, country) in [(2, "RS"), (1, "HR")] {
        let mut market = Market::new(
            mid(n),
            format!("Market {n}"),
            vec![CountryCode::new(country).unwrap()],
            Currency::EUR,
        )
        .unwrap();
        block_on(ports.markets().insert(&mut market)).unwrap();
    }
    let mut catalog = Catalog::new(cid(10), CatalogContext::Market(mid(1)), Some(plid(20)));
    block_on(ports.catalogs().insert(&mut catalog)).unwrap();
    block_on(ports.catalogs().add_products(cid(10), &[pid(1), pid(2)])).unwrap();
    let mut list = PriceList::new(plid(20), Currency::EUR, BasisPoints::new(-1000).unwrap());
    block_on(ports.price_lists().insert(&mut list)).unwrap();
    block_on(
        ports
            .price_lists()
            .set_fixed_prices(plid(20), &[(vid(1), eur(900))]),
    )
    .unwrap();
    ports
}

#[test]
fn markets_list_oldest_first_with_their_catalogs() {
    let ports = world();
    let views = block_on(list_markets(&ctx(vec![Scope::MarketsRead]), &ports)).unwrap();
    let summary: Vec<(MarketId, Option<CatalogId>)> =
        views.iter().map(|v| (v.market.id(), v.catalog)).collect();
    assert_eq!(summary, vec![(mid(1), Some(cid(10))), (mid(2), None)]);
}

#[test]
fn markets_list_alone_oldest_first() {
    let ports = world();
    let markets = block_on(list_markets_only(
        &ctx(vec![Scope::MarketsRead]),
        ports.markets(),
    ))
    .unwrap();
    let ids: Vec<MarketId> = markets.iter().map(|m| m.id()).collect();
    assert_eq!(ids, vec![mid(1), mid(2)]);
    assert_eq!(
        block_on(list_markets_only(&ctx(vec![]), ports.markets())),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MarketsRead
        )))
    );
}

#[test]
fn a_market_is_read_with_its_catalog_and_an_unknown_one_is_not_found() {
    let ports = world();
    let reader = ctx(vec![Scope::MarketsRead]);
    let MarketView { market, catalog } = block_on(get_market(&reader, &ports, mid(1))).unwrap();
    assert_eq!((market.id(), catalog), (mid(1), Some(cid(10))));
    assert_eq!(
        block_on(get_market(&reader, &ports, mid(99))),
        Err(CommandError::MarketNotFound(mid(99)))
    );
}

#[test]
fn market_reads_need_markets_read() {
    let ports = world();
    let nobody = ctx(vec![]);
    assert_eq!(
        block_on(list_markets(&nobody, &ports)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MarketsRead
        )))
    );
    assert_eq!(
        block_on(get_catalog(&nobody, ports.catalogs(), cid(10))),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MarketsRead
        )))
    );
}

#[test]
fn a_catalogs_products_come_in_pages() {
    let ports = world();
    let reader = ctx(vec![Scope::MarketsRead]);
    assert_eq!(
        block_on(list_catalog_products(
            &reader,
            ports.catalogs(),
            cid(10),
            first(1)
        )),
        Ok(Page {
            items: vec![pid(2)],
            has_more: true
        })
    );
    assert_eq!(
        block_on(list_catalog_products(
            &reader,
            ports.catalogs(),
            cid(99),
            first(1)
        )),
        Err(CommandError::CatalogNotFound(cid(99)))
    );
}

#[test]
fn price_lists_and_their_fixed_prices_need_pricing_read() {
    let ports = world();
    let reader = ctx(vec![Scope::PricingRead]);
    let lists = block_on(list_price_lists(&reader, ports.price_lists())).unwrap();
    assert_eq!(
        lists.iter().map(|l| l.id()).collect::<Vec<_>>(),
        vec![plid(20)]
    );
    let page = PageRequest {
        after: None,
        limit: PageLimit::DEFAULT,
    };
    assert_eq!(
        block_on(list_fixed_prices(
            &reader,
            ports.price_lists(),
            plid(20),
            page
        )),
        Ok(Page {
            items: vec![(vid(1), eur(900))],
            has_more: false
        })
    );
    assert_eq!(
        block_on(list_fixed_prices(
            &reader,
            ports.price_lists(),
            plid(99),
            page
        )),
        Err(CommandError::PriceListNotFound(plid(99)))
    );
    assert_eq!(
        block_on(list_price_lists(
            &ctx(vec![Scope::MarketsRead]),
            ports.price_lists()
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PricingRead
        )))
    );
}
