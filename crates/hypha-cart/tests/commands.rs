use std::collections::{HashMap, HashSet};

use hypha_cart::{
    AddLine, CartError, CartRepo, CommandError, PriceCartError, PricedCart, add_line, create_cart,
    get_cart,
};
use hypha_core::{
    Actor, AuthError, BasisPoints, CartId, CatalogId, CountryCode, Ctx, Currency, MarketId, Money,
    Price, PriceListId, ProductId, Scope, UserId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_pricing::PricingError;
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, SequentialIds,
};
use pollster::block_on;
use uuid::Uuid;

// IDs from 1000 up, so they never collide with SequentialIds (which counts from 1).
fn market_id() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1000))
}

fn catalog_id() -> CatalogId {
    CatalogId::from_uuid(Uuid::from_u128(1001))
}

fn list_id() -> PriceListId {
    PriceListId::from_uuid(Uuid::from_u128(1002))
}

/// Mug: active, in the catalog. Poster: active, not in the catalog. Draft: in the catalog, not active.
fn pid(name: &str) -> ProductId {
    let n = match name {
        "mug" => 1100,
        "poster" => 1101,
        _ => 1102,
    };
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn vid(name: &str) -> VariantId {
    let n = match name {
        "mug" => 1200,
        "poster" => 1201,
        _ => 1202,
    };
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::Eur)
}

struct World {
    carts: InMemoryCartRepo,
    products: InMemoryProductRepo,
    markets: InMemoryMarketRepo,
    catalogs: InMemoryCatalogRepo,
    price_lists: InMemoryPriceListRepo,
    ids: SequentialIds,
}

impl World {
    /// An EUR market whose catalog holds the mug and the draft, with a 0 bp EUR price list.
    fn new() -> Self {
        let world = Self {
            carts: InMemoryCartRepo::default(),
            products: InMemoryProductRepo::default(),
            markets: InMemoryMarketRepo::default(),
            catalogs: InMemoryCatalogRepo::default(),
            price_lists: InMemoryPriceListRepo::default(),
            ids: SequentialIds::default(),
        };

        let market = Market::new(
            market_id(),
            "Eurozone".to_string(),
            vec![CountryCode::new("HR").unwrap()],
            Currency::Eur,
        )
        .unwrap();
        block_on(world.markets.save(&market)).unwrap();

        for (name, active) in [("mug", true), ("poster", true), ("draft", false)] {
            let mut product = Product::new_simple(
                pid(name),
                Handle::new(name).unwrap(),
                name.to_string(),
                vid(name),
                Price::new(eur(1000)).unwrap(),
            );
            if active {
                product.set_status(ProductStatus::Active).unwrap();
            }
            block_on(world.products.save(&product)).unwrap();
        }

        let catalog = Catalog::new(
            catalog_id(),
            CatalogContext::Market(market_id()),
            HashSet::from([pid("mug"), pid("draft")]),
            Some(list_id()),
        );
        block_on(world.catalogs.save(&catalog)).unwrap();
        world.set_adjustment(0);
        world
    }

    fn set_adjustment(&self, bp: i32) {
        let list = PriceList::new(
            list_id(),
            Currency::Eur,
            BasisPoints::new(bp).unwrap(),
            HashMap::new(),
        )
        .unwrap();
        block_on(self.price_lists.save(&list)).unwrap();
    }

    fn create_cart(&self) -> CartId {
        block_on(create_cart(
            &Ctx::system(),
            &self.markets,
            &self.carts,
            &self.ids,
            market_id(),
        ))
        .unwrap()
        .id()
    }

    fn add(
        &self,
        ctx: &Ctx,
        cart: CartId,
        name: &str,
        quantity: u32,
    ) -> Result<PricedCart, CommandError> {
        block_on(add_line(
            ctx,
            &self.carts,
            &self.products,
            &self.markets,
            &self.catalogs,
            &self.price_lists,
            AddLine {
                cart,
                product: pid(name),
                variant: vid(name),
                quantity,
            },
        ))
    }

    fn get(&self, cart: CartId) -> Result<PricedCart, CommandError> {
        block_on(get_cart(
            &Ctx::system(),
            &self.carts,
            &self.products,
            &self.markets,
            &self.catalogs,
            &self.price_lists,
            cart,
        ))
    }

    fn stored_line_count(&self, cart: CartId) -> usize {
        block_on(self.carts.get(cart))
            .unwrap()
            .unwrap()
            .lines()
            .len()
    }
}

#[test]
fn create_cart_rejects_unknown_market() {
    let world = World::new();
    let unknown = MarketId::from_uuid(Uuid::from_u128(9999));
    let result = block_on(create_cart(
        &Ctx::system(),
        &world.markets,
        &world.carts,
        &world.ids,
        unknown,
    ));
    assert_eq!(result, Err(CommandError::MarketNotFound(unknown)));
}

#[test]
fn add_line_returns_priced_cart() {
    let world = World::new();
    let cart = world.create_cart();

    let priced = world.add(&Ctx::system(), cart, "mug", 2).unwrap();

    assert_eq!(priced.lines.len(), 1);
    assert_eq!(priced.lines[0].unit_price.money(), eur(1000));
    assert_eq!(priced.total, eur(2000));
}

#[test]
fn add_line_merges_same_variant() {
    let world = World::new();
    let cart = world.create_cart();

    world.add(&Ctx::system(), cart, "mug", 2).unwrap();
    let priced = world.add(&Ctx::system(), cart, "mug", 1).unwrap();

    assert_eq!(priced.lines.len(), 1);
    assert_eq!(priced.total, eur(3000));
}

#[test]
fn get_cart_reprices_after_price_list_change() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();

    world.set_adjustment(-1000);

    assert_eq!(world.get(cart).unwrap().total, eur(1800));
}

#[test]
fn add_line_rejects_product_not_in_catalog_and_stores_nothing() {
    let world = World::new();
    let cart = world.create_cart();

    assert_eq!(
        world.add(&Ctx::system(), cart, "poster", 1),
        Err(CommandError::Price(PriceCartError::Pricing(
            PricingError::NotAvailable {
                product: pid("poster"),
                market: market_id(),
            }
        )))
    );
    assert_eq!(world.stored_line_count(cart), 0);
}

#[test]
fn add_line_rejects_inactive_product() {
    let world = World::new();
    let cart = world.create_cart();

    assert_eq!(
        world.add(&Ctx::system(), cart, "draft", 1),
        Err(CommandError::Price(PriceCartError::ProductNotActive(pid(
            "draft"
        ))))
    );
}

#[test]
fn add_line_rejects_zero_quantity() {
    let world = World::new();
    let cart = world.create_cart();

    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 0),
        Err(CommandError::Cart(CartError::ZeroQuantity))
    );
}

#[test]
fn add_line_rejects_unknown_cart() {
    let world = World::new();
    let unknown = CartId::from_uuid(Uuid::from_u128(9999));

    assert_eq!(
        world.add(&Ctx::system(), unknown, "mug", 1),
        Err(CommandError::CartNotFound(unknown))
    );
}

#[test]
fn add_line_rejects_missing_write_scope_and_stores_nothing() {
    let world = World::new();
    let cart = world.create_cart();
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::CartsRead],
    );

    assert_eq!(
        world.add(&reader, cart, "mug", 1),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::CartsWrite
        )))
    );
    assert_eq!(world.stored_line_count(cart), 0);
}
