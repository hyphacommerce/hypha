#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]

use hypha_categories::{Category, CategoryRepo, NewCategory};
use hypha_core::{
    BasisPoints, CatalogId, CategoryId, CountryCode, Currency, Handle, MarketId, Money, Price,
    PriceListId, ProductId, Quantity, VariantId,
};
use hypha_markets::{
    Adjustment, AppliesTo, Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList,
    PriceListDraft, PriceListRepo,
};
use hypha_pricing::{PricingContext, VariantPrices, load_pricing, resolve_price};
use hypha_testkit::InMemoryPorts;
use pollster::block_on;
use uuid::Uuid;

fn market_id() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1))
}

fn category() -> CategoryId {
    CategoryId::from_uuid(Uuid::from_u128(2))
}

fn mug() -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(10))
}

fn mug_v() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(11))
}

fn plate() -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(20))
}

fn plate_v() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(21))
}

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap()
}

/// A market whose catalog holds the mug and the plate, priced by a list taking 10% off
/// the category the mug alone is in.
fn store() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let mut market = Market::new(
        market_id(),
        "Croatia".to_string(),
        vec![CountryCode::new("HR").unwrap()],
        Currency::EUR,
    )
    .unwrap();
    block_on(ports.markets.insert(&mut market)).unwrap();

    let mut c = Category::new(NewCategory {
        id: category(),
        name: "Mugs".to_string(),
        handle: Handle::new("mugs").unwrap(),
        description: None,
        parent: None,
        position: 0,
        visible: true,
    })
    .unwrap();
    block_on(ports.categories.insert(&mut c)).unwrap();
    block_on(ports.categories.add_products(category(), &[mug()])).unwrap();

    let list = PriceListId::from_uuid(Uuid::from_u128(3));
    let mut prices = PriceList::new(list, PriceListDraft::new("Mugs sale", Currency::EUR)).unwrap();
    prices.set_adjustment(Some(
        Adjustment::new(
            BasisPoints::new(-1000).unwrap(),
            AppliesTo::Categories(vec![category()]),
        )
        .unwrap(),
    ));
    block_on(ports.price_lists.insert(&mut prices)).unwrap();

    let catalog_id = CatalogId::from_uuid(Uuid::from_u128(4));
    let mut catalog = Catalog::new(catalog_id, CatalogContext::Market(market_id()), Some(list));
    block_on(ports.catalogs.insert(&mut catalog)).unwrap();
    block_on(ports.catalogs.add_products(catalog_id, &[mug(), plate()])).unwrap();
    ports
}

fn price(pricing: &PricingContext, product: ProductId, variant: VariantId) -> Price {
    let variant = VariantPrices {
        product,
        variant,
        base: eur(1000),
        compare_at: None,
    };
    resolve_price(pricing, variant, Quantity::new(1).unwrap())
        .unwrap()
        .price
}

#[test]
fn an_unknown_market_loads_nothing() {
    let ports = store();
    let other = MarketId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(load_pricing(&ports, other, &[mug()], &[mug_v()], &[])).map(|p| p.is_some()),
        Ok(false)
    );
}

#[test]
fn a_category_adjustment_reaches_only_products_in_the_category() {
    let ports = store();
    let pricing = block_on(load_pricing(
        &ports,
        market_id(),
        &[mug(), plate()],
        &[mug_v(), plate_v()],
        &[],
    ))
    .unwrap()
    .unwrap();
    assert_eq!(price(&pricing, mug(), mug_v()), eur(900));
    assert_eq!(price(&pricing, plate(), plate_v()), eur(1000));
}
