use std::collections::{HashMap, HashSet};

use hypha_core::{
    Actor, AuthError, BasisPoints, CatalogId, CountryCode, Ctx, Currency, MarketId, PriceListId,
    Scope, UserId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, CommandError, Market, MarketRepo, PriceList,
    PriceListRepo, assign_price_list,
};
use hypha_testkit::{InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo};
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

/// Repos with an EUR market, its catalog (no price list), an EUR list (10) and an RSD list (11).
struct World {
    markets: InMemoryMarketRepo,
    catalogs: InMemoryCatalogRepo,
    price_lists: InMemoryPriceListRepo,
}

impl World {
    fn new() -> Self {
        let world = Self {
            markets: InMemoryMarketRepo::default(),
            catalogs: InMemoryCatalogRepo::default(),
            price_lists: InMemoryPriceListRepo::default(),
        };
        let market = Market::new(
            market_id(),
            "Eurozone".to_string(),
            vec![CountryCode::new("HR").unwrap()],
            Currency::Eur,
        )
        .unwrap();
        let catalog = Catalog::new(
            catalog_id(),
            CatalogContext::Market(market_id()),
            HashSet::new(),
            None,
        );
        block_on(world.markets.save(&market)).unwrap();
        block_on(world.catalogs.save(&catalog)).unwrap();
        for (n, currency) in [(10, Currency::Eur), (11, Currency::Rsd)] {
            let list =
                PriceList::new(list_id(n), currency, BasisPoints::ZERO, HashMap::new()).unwrap();
            block_on(world.price_lists.save(&list)).unwrap();
        }
        world
    }

    fn assign(
        &self,
        ctx: &Ctx,
        catalog: CatalogId,
        list: Option<PriceListId>,
    ) -> Result<Catalog, CommandError> {
        block_on(assign_price_list(
            ctx,
            &self.markets,
            &self.catalogs,
            &self.price_lists,
            catalog,
            list,
        ))
    }

    fn stored_price_list(&self) -> Option<PriceListId> {
        block_on(self.catalogs.get(catalog_id()))
            .unwrap()
            .unwrap()
            .price_list()
    }
}

#[test]
fn assigns_list_in_market_currency() {
    let world = World::new();
    let catalog = world
        .assign(&Ctx::system(), catalog_id(), Some(list_id(10)))
        .unwrap();
    assert_eq!(catalog.price_list(), Some(list_id(10)));
    assert_eq!(world.stored_price_list(), Some(list_id(10)));
}

#[test]
fn rejects_list_in_other_currency_and_stores_nothing() {
    let world = World::new();
    assert_eq!(
        world.assign(&Ctx::system(), catalog_id(), Some(list_id(11))),
        Err(CommandError::CurrencyMismatch {
            expected: Currency::Eur,
            found: Currency::Rsd,
        })
    );
    assert_eq!(world.stored_price_list(), None);
}

#[test]
fn unassigns_list_with_none() {
    let world = World::new();
    world
        .assign(&Ctx::system(), catalog_id(), Some(list_id(10)))
        .unwrap();
    world.assign(&Ctx::system(), catalog_id(), None).unwrap();
    assert_eq!(world.stored_price_list(), None);
}

#[test]
fn rejects_unknown_catalog() {
    let world = World::new();
    let unknown = CatalogId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        world.assign(&Ctx::system(), unknown, Some(list_id(10))),
        Err(CommandError::CatalogNotFound(unknown))
    );
}

#[test]
fn rejects_unknown_price_list() {
    let world = World::new();
    assert_eq!(
        world.assign(&Ctx::system(), catalog_id(), Some(list_id(99))),
        Err(CommandError::PriceListNotFound(list_id(99)))
    );
}

#[test]
fn rejects_catalog_whose_market_is_missing() {
    let world = World::new();
    let orphan_market = MarketId::from_uuid(Uuid::from_u128(98));
    let orphan = Catalog::new(
        CatalogId::from_uuid(Uuid::from_u128(97)),
        CatalogContext::Market(orphan_market),
        HashSet::new(),
        None,
    );
    block_on(world.catalogs.save(&orphan)).unwrap();
    assert_eq!(
        world.assign(&Ctx::system(), orphan.id(), Some(list_id(10))),
        Err(CommandError::MarketNotFound(orphan_market))
    );
}

#[test]
fn rejects_missing_write_scope_and_stores_nothing() {
    let world = World::new();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::MarketsRead],
    );
    assert_eq!(
        world.assign(&reader, catalog_id(), Some(list_id(10))),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::MarketsWrite
        )))
    );
    assert_eq!(world.stored_price_list(), None);
}
