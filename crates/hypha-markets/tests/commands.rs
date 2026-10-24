use hypha_core::{
    Actor, AuthError, BasisPoints, CatalogId, CountryCode, Ctx, Currency, MarketId, PriceListId,
    Scope, UserId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, CommandError, Market, MarketError, MarketRepo, PriceList,
    PriceListDraft, PriceListRepo, add_catalog_price_list, delete_price_list,
    remove_catalog_price_list, set_catalog_price_lists,
};
use hypha_testkit::InMemoryPorts;
use pollster::block_on;
use uuid::Uuid;

fn market_id() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1))
}

fn catalog_id() -> CatalogId {
    CatalogId::from_uuid(Uuid::from_u128(2))
}

fn list_id(n: u128) -> PriceListId {
    PriceListId::from_uuid(Uuid::from_u128(n))
}

/// Ports with an EUR market, its catalog (no price lists), EUR lists 10 and 12 and an
/// RSD list 11.
fn world() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let mut market = Market::new(
        market_id(),
        "Eurozone".to_string(),
        vec![CountryCode::new("HR").unwrap()],
        Currency::EUR,
    )
    .unwrap();
    let mut catalog = Catalog::new(catalog_id(), CatalogContext::Market(market_id()), None);
    block_on(ports.markets.insert(&mut market)).unwrap();
    block_on(ports.catalogs.insert(&mut catalog)).unwrap();
    for (n, currency) in [
        (10, Currency::EUR),
        (11, Currency::RSD),
        (12, Currency::EUR),
    ] {
        let mut list = PriceList::new(
            list_id(n),
            PriceListDraft::new(&format!("List {n}"), currency).adjusted(BasisPoints::ZERO),
        )
        .unwrap();
        block_on(ports.price_lists.insert(&mut list)).unwrap();
    }
    ports
}

fn set(ports: &InMemoryPorts, ctx: &Ctx, lists: Vec<PriceListId>) -> Result<Catalog, CommandError> {
    block_on(set_catalog_price_lists(ctx, ports, catalog_id(), lists))
}

fn stored(ports: &InMemoryPorts) -> Vec<PriceListId> {
    block_on(ports.catalogs.get(catalog_id()))
        .unwrap()
        .unwrap()
        .price_lists()
        .to_vec()
}

#[test]
fn sets_lists_in_the_market_currency() {
    let ports = world();
    let catalog = set(&ports, &Ctx::system(), vec![list_id(12), list_id(10)]).unwrap();
    assert_eq!(catalog.price_lists(), [list_id(10), list_id(12)]);
    assert_eq!(stored(&ports), [list_id(10), list_id(12)]);
    assert!(
        ports
            .outbox
            .types()
            .contains(&"catalog.price_lists_added".to_string())
    );
}

#[test]
fn rejects_a_list_in_another_currency_and_stores_nothing() {
    let ports = world();
    assert_eq!(
        set(&ports, &Ctx::system(), vec![list_id(10), list_id(11)]),
        Err(CommandError::CurrencyMismatch {
            expected: Currency::EUR,
            found: Currency::RSD,
        })
    );
    assert_eq!(stored(&ports), []);
}

#[test]
fn rejects_a_list_listed_twice() {
    let ports = world();
    assert_eq!(
        set(&ports, &Ctx::system(), vec![list_id(10), list_id(10)]),
        Err(CommandError::Market(MarketError::DuplicatePriceList(
            list_id(10)
        )))
    );
}

#[test]
fn adds_and_removes_one_list() {
    let ports = world();
    block_on(add_catalog_price_list(
        &Ctx::system(),
        &ports,
        catalog_id(),
        list_id(10),
    ))
    .unwrap();
    block_on(add_catalog_price_list(
        &Ctx::system(),
        &ports,
        catalog_id(),
        list_id(12),
    ))
    .unwrap();
    block_on(remove_catalog_price_list(
        &Ctx::system(),
        &ports,
        catalog_id(),
        list_id(10),
    ))
    .unwrap();
    assert_eq!(stored(&ports), [list_id(12)]);
    assert_eq!(
        block_on(remove_catalog_price_list(
            &Ctx::system(),
            &ports,
            catalog_id(),
            list_id(10),
        )),
        Err(CommandError::Market(MarketError::PriceListNotInCatalog(
            list_id(10)
        )))
    );
}

#[test]
fn rejects_unknown_catalog() {
    let ports = world();
    let unknown = CatalogId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(set_catalog_price_lists(
            &Ctx::system(),
            &ports,
            unknown,
            vec![list_id(10)]
        )),
        Err(CommandError::CatalogNotFound(unknown))
    );
}

#[test]
fn rejects_unknown_price_list() {
    let ports = world();
    assert_eq!(
        set(&ports, &Ctx::system(), vec![list_id(99)]),
        Err(CommandError::PriceListNotFound(list_id(99)))
    );
}

#[test]
fn rejects_catalog_whose_market_is_missing() {
    let ports = world();
    let orphan_market = MarketId::from_uuid(Uuid::from_u128(98));
    let mut orphan = Catalog::new(
        CatalogId::from_uuid(Uuid::from_u128(97)),
        CatalogContext::Market(orphan_market),
        None,
    );
    block_on(ports.catalogs.insert(&mut orphan)).unwrap();
    assert_eq!(
        block_on(set_catalog_price_lists(
            &Ctx::system(),
            &ports,
            orphan.id(),
            vec![list_id(10)]
        )),
        Err(CommandError::MarketNotFound(orphan_market))
    );
}

#[test]
fn rejects_missing_pricing_write_scope_and_stores_nothing() {
    let ports = world();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::MarketsWrite, Scope::PricingRead],
    );
    assert_eq!(
        set(&ports, &reader, vec![list_id(10)]),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PricingWrite
        )))
    );
    assert_eq!(stored(&ports), []);
}

#[test]
fn a_list_on_a_catalog_is_not_deleted() {
    let ports = world();
    set(&ports, &Ctx::system(), vec![list_id(10)]).unwrap();
    assert_eq!(
        block_on(delete_price_list(&Ctx::system(), &ports, list_id(10))),
        Err(CommandError::PriceListInUse {
            price_list: list_id(10),
            catalogs: vec![catalog_id()],
        })
    );
    block_on(delete_price_list(&Ctx::system(), &ports, list_id(12))).unwrap();
    assert_eq!(block_on(ports.price_lists.get(list_id(12))), Ok(None));
    assert_eq!(
        ports.outbox.types().last().map(String::as_str),
        Some("price_list.deleted")
    );
    assert_eq!(
        block_on(delete_price_list(&Ctx::system(), &ports, list_id(12))),
        Err(CommandError::PriceListNotFound(list_id(12)))
    );
}
