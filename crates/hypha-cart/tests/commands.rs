use hypha_cart::{
    AddLine, CartError, CartPorts, CartRepo, CommandError, LineError, LineStatus, PricedCart,
    add_line, create_cart, get_cart,
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

/// Mug and cup: active, in the catalog. Poster: active, not in the catalog. Draft: in the catalog, not active.
fn pid(name: &str) -> ProductId {
    let n = match name {
        "mug" => 1100,
        "poster" => 1101,
        "cup" => 1103,
        "draft" => 1102,
        _ => 1199,
    };
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn vid(name: &str) -> VariantId {
    let n = match name {
        "mug" => 1200,
        "poster" => 1201,
        "cup" => 1203,
        "draft" => 1202,
        _ => 1299,
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
    /// An EUR market whose catalog holds the mug, the cup and the draft, with a 0 bp EUR price list.
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

        for (name, active) in [
            ("mug", true),
            ("cup", true),
            ("poster", true),
            ("draft", false),
        ] {
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
            Some(list_id()),
        );
        block_on(world.catalogs.save(&catalog)).unwrap();
        block_on(
            world
                .catalogs
                .add_products(catalog_id(), &[pid("mug"), pid("cup"), pid("draft")]),
        )
        .unwrap();
        world.set_adjustment(0);
        world
    }

    fn set_adjustment(&self, bp: i32) {
        let list = PriceList::new(list_id(), Currency::Eur, BasisPoints::new(bp).unwrap());
        block_on(self.price_lists.save(&list)).unwrap();
    }

    fn create_cart(&self) -> CartId {
        block_on(create_cart(&Ctx::system(), self, &self.ids, market_id()))
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
            self,
            AddLine {
                cart,
                variant: vid(name),
                quantity,
            },
        ))
    }

    fn get(&self, cart: CartId) -> Result<PricedCart, CommandError> {
        block_on(get_cart(&Ctx::system(), self, cart))
    }

    fn archive(&self, name: &str) {
        let mut product = block_on(self.products.get(pid(name))).unwrap().unwrap();
        product.set_status(ProductStatus::Archived).unwrap();
        block_on(self.products.save(&product)).unwrap();
    }

    fn stored_line_count(&self, cart: CartId) -> usize {
        block_on(self.carts.get(cart))
            .unwrap()
            .unwrap()
            .lines()
            .len()
    }
}

impl CartPorts for World {
    type Carts = InMemoryCartRepo;
    type Products = InMemoryProductRepo;
    type Markets = InMemoryMarketRepo;
    type Catalogs = InMemoryCatalogRepo;
    type PriceLists = InMemoryPriceListRepo;

    fn carts(&self) -> &Self::Carts {
        &self.carts
    }
    fn products(&self) -> &Self::Products {
        &self.products
    }
    fn markets(&self) -> &Self::Markets {
        &self.markets
    }
    fn catalogs(&self) -> &Self::Catalogs {
        &self.catalogs
    }
    fn price_lists(&self) -> &Self::PriceLists {
        &self.price_lists
    }
}

#[test]
fn create_cart_rejects_unknown_market() {
    let world = World::new();
    let unknown = MarketId::from_uuid(Uuid::from_u128(9999));
    let result = block_on(create_cart(&Ctx::system(), &world, &world.ids, unknown));
    assert_eq!(result, Err(CommandError::MarketNotFound(unknown)));
}

#[test]
fn add_line_returns_priced_cart() {
    let world = World::new();
    let cart = world.create_cart();

    let priced = world.add(&Ctx::system(), cart, "mug", 2).unwrap();

    assert_eq!(priced.lines.len(), 1);
    assert_eq!(priced.lines[0].product, pid("mug"));
    assert_eq!(
        priced.lines[0].status,
        LineStatus::Priced {
            unit_price: Price::new(eur(1000)).unwrap(),
            total: eur(2000),
        }
    );
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
        Err(CommandError::Line(LineError::Pricing(
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
        Err(CommandError::Line(LineError::ProductNotActive(pid(
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

#[test]
fn add_line_rejects_unknown_variant() {
    let world = World::new();
    let cart = world.create_cart();

    assert_eq!(
        world.add(&Ctx::system(), cart, "unknown", 1),
        Err(CommandError::VariantNotFound(vid("unknown")))
    );
}

#[test]
fn get_cart_marks_stale_line_unavailable_and_keeps_the_rest() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();
    world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    world.archive("mug");
    let priced = world.get(cart).unwrap();

    assert_eq!(
        priced.lines[0].status,
        LineStatus::Unavailable(LineError::ProductNotActive(pid("mug")))
    );
    assert!(priced.lines[1].is_available());
    assert_eq!(priced.total, eur(1000));
    assert!(!priced.is_fully_available());
}

#[test]
fn add_line_still_works_when_another_line_is_stale() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();
    world.archive("mug");

    let priced = world.add(&Ctx::system(), cart, "cup", 3).unwrap();

    assert_eq!(priced.lines.len(), 2);
    assert_eq!(priced.total, eur(3000));
}

#[test]
fn uses_fixed_price_from_price_list() {
    let world = World::new();
    let cart = world.create_cart();
    let fixed = Price::new(eur(450)).unwrap();
    block_on(
        world
            .price_lists
            .set_fixed_prices(list_id(), &[(vid("cup"), fixed)]),
    )
    .unwrap();

    let priced = world.add(&Ctx::system(), cart, "cup", 2).unwrap();

    assert_eq!(priced.total, eur(900));
}

#[test]
fn line_becomes_unavailable_when_product_leaves_catalog() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();

    block_on(world.catalogs.remove_products(catalog_id(), &[pid("mug")])).unwrap();
    let priced = world.get(cart).unwrap();

    assert_eq!(
        priced.lines[0].status,
        LineStatus::Unavailable(LineError::Pricing(PricingError::NotAvailable {
            product: pid("mug"),
            market: market_id(),
        }))
    );
    assert_eq!(priced.total, eur(0));
}
