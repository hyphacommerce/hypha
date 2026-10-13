use hypha_cart::{
    AddLine, CartError, CartPorts, CartRepo, CommandError, LineError, LineStatus, PricedCart,
    add_line, create_cart, get_cart,
};
use hypha_core::{
    Actor, AuthError, BasisPoints, CartId, CatalogId, CountryCode, Ctx, Currency, MarketId, Money,
    Price, PriceListId, ProductId, Scope, TaxClassId, TaxRate, TaxRegionId, UserId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_pricing::PricingError;
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_tax::{TaxClass, TaxClassRepo, TaxLine, TaxRegion, TaxRegionRepo, TaxSource};
use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo, SequentialIds,
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

fn region_id() -> TaxRegionId {
    TaxRegionId::from_uuid(Uuid::from_u128(1003))
}

fn books() -> TaxClassId {
    TaxClassId::from_uuid(Uuid::from_u128(1004))
}

fn hr() -> CountryCode {
    CountryCode::new("HR").unwrap()
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
    Money::new(amount, Currency::Eur).unwrap()
}

struct World {
    carts: InMemoryCartRepo,
    products: InMemoryProductRepo,
    markets: InMemoryMarketRepo,
    catalogs: InMemoryCatalogRepo,
    price_lists: InMemoryPriceListRepo,
    tax_classes: InMemoryTaxClassRepo,
    tax_regions: InMemoryTaxRegionRepo,
    ids: SequentialIds,
}

impl World {
    /// An EUR market for Croatia whose catalog holds the mug, the cup and the draft, with a
    /// 0 bp EUR price list. No tax region yet, so nothing is taxed until `tax_croatia`.
    fn new() -> Self {
        let world = Self {
            carts: InMemoryCartRepo::default(),
            products: InMemoryProductRepo::default(),
            markets: InMemoryMarketRepo::default(),
            catalogs: InMemoryCatalogRepo::default(),
            price_lists: InMemoryPriceListRepo::default(),
            tax_classes: InMemoryTaxClassRepo::default(),
            tax_regions: InMemoryTaxRegionRepo::default(),
            ids: SequentialIds::default(),
        };

        let mut market = Market::new(
            market_id(),
            "Eurozone".to_string(),
            vec![hr()],
            Currency::Eur,
        )
        .unwrap();
        block_on(world.markets.insert(&mut market)).unwrap();

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
            block_on(world.products.insert(&mut product)).unwrap();
        }

        let mut catalog = Catalog::new(
            catalog_id(),
            CatalogContext::Market(market_id()),
            Some(list_id()),
        );
        block_on(world.catalogs.insert(&mut catalog)).unwrap();
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
        let bp = BasisPoints::new(bp).unwrap();
        match block_on(self.price_lists.get(list_id())).unwrap() {
            Some(mut list) => {
                list.set_adjustment(bp);
                block_on(self.price_lists.update(&mut list)).unwrap();
            }
            None => {
                let mut list = PriceList::new(list_id(), Currency::Eur, bp);
                block_on(self.price_lists.insert(&mut list)).unwrap();
            }
        }
    }

    /// Croatia: 25% by default, books (the mug, here) 5%.
    fn tax_croatia(&self) {
        let mut class = TaxClass::new(books(), "Books".to_string());
        block_on(self.tax_classes.insert(&mut class)).unwrap();
        block_on(self.tax_classes.assign(&[pid("mug")], Some(books()))).unwrap();

        let mut region = TaxRegion::new(region_id(), hr(), TaxRate::new(250_000).unwrap());
        region.set_class_rate(books(), TaxRate::new(50_000).unwrap());
        block_on(self.tax_regions.insert(&mut region)).unwrap();
    }

    fn set_prices_include_tax(&self) {
        let mut market = block_on(self.markets.get(market_id())).unwrap().unwrap();
        market.set_prices_include_tax(true);
        block_on(self.markets.update(&mut market)).unwrap();
    }

    fn create_cart(&self) -> CartId {
        block_on(create_cart(&Ctx::system(), self, &self.ids, hr()))
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
        block_on(self.products.update(&mut product)).unwrap();
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
    type TaxClasses = InMemoryTaxClassRepo;
    type TaxRegions = InMemoryTaxRegionRepo;

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
    fn tax_classes(&self) -> &Self::TaxClasses {
        &self.tax_classes
    }
    fn tax_regions(&self) -> &Self::TaxRegions {
        &self.tax_regions
    }
}

#[test]
fn create_cart_picks_the_market_of_its_country() {
    let world = World::new();
    let cart = block_on(create_cart(&Ctx::system(), &world, &world.ids, hr())).unwrap();
    assert_eq!((cart.market(), cart.country()), (market_id(), hr()));
    assert_eq!(block_on(world.carts.get(cart.id())), Ok(Some(cart)));
}

#[test]
fn create_cart_rejects_country_without_market() {
    let world = World::new();
    let de = CountryCode::new("DE").unwrap();
    let result = block_on(create_cart(&Ctx::system(), &world, &world.ids, de));
    assert_eq!(result, Err(CommandError::NoMarketForCountry(de)));
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
            tax: None,
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

// ---- tax ----

fn line_tax(priced: &PricedCart, line: usize) -> Option<TaxLine> {
    match &priced.lines[line].status {
        LineStatus::Priced { tax, .. } => *tax,
        LineStatus::Unavailable(_) => None,
    }
}

#[test]
fn tax_is_added_per_line_at_the_class_or_default_rate() {
    let world = World::new();
    world.tax_croatia();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();

    let priced = world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    // The mug is a book: 2000 × 5% = 100. The cup pays the default: 1000 × 25% = 250.
    assert_eq!(
        line_tax(&priced, 0),
        Some(TaxLine {
            rate: TaxRate::new(50_000).unwrap(),
            class: Some(books()),
            amount: eur(100),
            source: TaxSource::RateTable {
                region: region_id()
            },
        })
    );
    assert_eq!(line_tax(&priced, 1).map(|t| t.amount), Some(eur(250)));
    assert_eq!(
        (priced.subtotal, priced.tax_total, priced.total),
        (eur(3000), eur(350), eur(3350))
    );
}

#[test]
fn tax_is_extracted_when_the_market_includes_it() {
    let world = World::new();
    world.tax_croatia();
    world.set_prices_include_tax();
    let cart = world.create_cart();

    let priced = world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    // 1000 includes 25%: 200 of it is tax, and the buyer pays 1000.
    assert!(priced.taxes_included);
    assert_eq!(
        (priced.subtotal, priced.tax_total, priced.total),
        (eur(1000), eur(200), eur(1000))
    );
}

#[test]
fn country_without_tax_region_is_not_taxed() {
    let world = World::new();
    let cart = world.create_cart();

    let priced = world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    assert_eq!(line_tax(&priced, 0), None);
    assert_eq!((priced.tax_total, priced.total), (eur(0), eur(1000)));
}
