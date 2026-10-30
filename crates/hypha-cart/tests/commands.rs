#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]

use hypha_cart::{
    AddLine, CartError, CartRepo, CommandError, CreateCart, LineError, LineStatus, PricedCart,
    SetLineQuantity, add_discount_code, add_line, create_cart, get_cart, remove_cart_line,
    remove_discount_code, set_cart_customer, set_cart_email, set_line_quantity,
};
use hypha_channels::ChannelRepo;
use hypha_core::{
    Actor, AuthError, BasisPoints, CartId, CatalogId, CountryCode, CountryError, Ctx, Currency,
    InventoryItemId, LocationId, MarketId, Money, Price, PriceListId, ProductId, Scope, TaxClassId,
    TaxRate, TaxRegionId, UserId, VariantId,
};
use hypha_core::{
    CustomerId, Email, EmailError, IdempotencyError, IdempotencyRequest, Idempotent, MoneyError,
    Quantity, QuantityError,
};
use hypha_customers::{Customer, CustomerRepo};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_pricing::PricingError;
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_tax::{TaxClass, TaxClassRepo, TaxLine, TaxRegion, TaxRegionRepo, TaxSource};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
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

/// The warehouse that serves the market (`stock` creates it on first use).
fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1005))
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

fn qty(n: u32) -> Quantity {
    Quantity::new(n).unwrap()
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
    Money::new(amount, Currency::EUR).unwrap()
}

/// The repositories (`InMemoryPorts`, reached through `Deref`), an ID generator and a
/// clock.
struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
}

impl std::ops::Deref for World {
    type Target = InMemoryPorts;
    fn deref(&self) -> &InMemoryPorts {
        &self.ports
    }
}

impl World {
    /// An EUR market for Croatia whose catalog holds the mug, the cup and the draft, with a
    /// 0 bp EUR price list. No tax region yet, so nothing is taxed until `tax_croatia`.
    fn new() -> Self {
        let world = Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
        };

        let mut market = Market::new(
            market_id(),
            "Eurozone".to_string(),
            vec![hr()],
            Currency::EUR,
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
            )
            .unwrap();
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
                list.set_adjustment(Some(hypha_markets::Adjustment::all(bp)));
                block_on(self.price_lists.update(&mut list)).unwrap();
            }
            None => {
                let mut list = PriceList::new(
                    list_id(),
                    hypha_markets::PriceListDraft::new("Euro", Currency::EUR).adjusted(bp),
                )
                .unwrap();
                block_on(self.price_lists.insert(&mut list)).unwrap();
            }
        }
    }

    /// Croatia: 25% by default, books (the mug, here) 5%.
    fn tax_croatia(&self) {
        let mut class = TaxClass::new(books(), "Books".to_string()).unwrap();
        block_on(self.tax_classes.insert(&mut class)).unwrap();
        block_on(self.tax_classes.assign(&[pid("mug")], Some(books()))).unwrap();

        let mut region = TaxRegion::new(region_id(), hr(), TaxRate::new(250_000).unwrap());
        region.set_class_rate(books(), TaxRate::new(50_000).unwrap());
        block_on(self.tax_regions.insert(&mut region)).unwrap();
    }

    /// Tracks `name`'s stock: `on_hand` at the warehouse, which serves the market.
    /// Products without a call to this are untracked and never run out.
    fn stock(&self, name: &str, on_hand: i64) {
        if block_on(self.locations.get(warehouse())).unwrap().is_none() {
            let mut warehouse =
                Location::new(warehouse(), "Zagreb".to_string(), vec![market_id()], 0).unwrap();
            block_on(self.locations.insert(&mut warehouse)).unwrap();
        }
        let id = InventoryItemId::from_uuid(vid(name).as_uuid());
        let mut item = InventoryItem::new(id, vid(name), true, false);
        item.set_on_hand(warehouse(), on_hand, None).unwrap();
        block_on(self.inventory_items.insert(&mut item)).unwrap();
    }

    /// Someone else's checkout takes `quantity` of `name`.
    fn sell_elsewhere(&self, name: &str, quantity: u32) {
        let mut item = block_on(self.inventory_items.get_by_variant(vid(name)))
            .unwrap()
            .unwrap();
        let location = block_on(self.locations.get(warehouse())).unwrap().unwrap();
        item.reserve(
            &ReservationRef::new("elsewhere").unwrap(),
            qty(quantity),
            &[location],
        )
        .unwrap();
        block_on(self.inventory_items.update(&mut item)).unwrap();
    }

    fn set_prices_include_tax(&self) {
        let mut market = block_on(self.markets.get(market_id())).unwrap().unwrap();
        market.set_prices_include_tax(true);
        block_on(self.markets.update(&mut market)).unwrap();
    }

    fn create_cart(&self) -> CartId {
        self.create_cart_in("HR").unwrap().id
    }

    fn create_cart_in(&self, country: &str) -> Result<PricedCart, CommandError> {
        let input = CreateCart {
            country: country.to_string(),
            idempotency: None,
            customer: None,
        };
        block_on(create_cart(
            &self.ports.storefront(),
            &self.ports,
            &self.ids,
            &self.clock,
            input,
        ))
        .map(|created| created.value)
    }

    fn add(
        &self,
        ctx: &Ctx,
        cart: CartId,
        name: &str,
        quantity: u32,
    ) -> Result<PricedCart, CommandError> {
        let input = AddLine {
            cart,
            variant: vid(name),
            quantity,
            idempotency: None,
        };
        block_on(add_line(ctx, &self.ports, &self.clock, input)).map(|added| added.value)
    }

    fn get(&self, cart: CartId) -> Result<PricedCart, CommandError> {
        block_on(get_cart(&Ctx::system(), &self.ports, cart))
    }

    fn archive(&self, name: &str) {
        let mut product = block_on(self.products.get(pid(name))).unwrap().unwrap();
        product.set_status(ProductStatus::Archived).unwrap();
        block_on(self.products.update(&mut product)).unwrap();
    }

    /// Deletes the product as `hypha_merchandising::delete_product` does.
    fn delete_product(&self, name: &str) {
        use hypha_core::HasBatch;
        use hypha_products::StageProducts;
        let mut product = block_on(self.products.get(pid(name))).unwrap().unwrap();
        product.delete();
        let mut batch = self.batch();
        batch.delete_product(&mut product);
        block_on(self.commit(batch)).unwrap();
    }

    fn stored_quantity(&self, cart: CartId, name: &str) -> u32 {
        block_on(self.carts.get(cart))
            .unwrap()
            .unwrap()
            .line(vid(name))
            .unwrap()
            .quantity()
            .value()
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
fn create_cart_picks_the_market_of_its_country() {
    let world = World::new();
    let cart = world.create_cart_in("HR").unwrap();
    assert_eq!((cart.country, cart.lines.len()), (hr(), 0));
    let stored = block_on(world.carts.get(cart.id)).unwrap().unwrap();
    assert_eq!((stored.market(), stored.country()), (market_id(), hr()));
    assert_eq!(world.get(cart.id), Ok(cart));
}

/// What an empty cart in the Croatian market prices to, whatever else is set up.
fn empty_in_croatia(id: CartId, taxes_included: bool) -> PricedCart {
    PricedCart {
        id,
        market: market_id(),
        currency: Currency::EUR,
        country: hr(),
        email: None,
        customer: None,
        order: None,
        lines: vec![],
        taxes_included,
        subtotal: eur(0),
        shipping_address: None,
        shipping_groups: vec![],
        shipping_total: eur(0),
        discounts: vec![],
        discount_total: eur(0),
        discount_codes: vec![],
        tax_total: eur(0),
        total: eur(0),
        payment_method: None,
        marketing_opt_in: false,
    }
}

#[test]
fn a_new_cart_is_priced_from_its_market() {
    let world = World::new();
    world.tax_croatia();
    world.stock("mug", 2);
    world.set_prices_include_tax();
    let cart = world.create_cart_in("HR").unwrap();
    assert_eq!(cart, empty_in_croatia(cart.id, true));
    assert_eq!(world.get(cart.id), Ok(cart));
}

#[test]
fn an_emptied_cart_prices_like_a_new_one() {
    let world = World::new();
    world.tax_croatia();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();
    let emptied = block_on(remove_cart_line(
        &Ctx::system(),
        &world.ports,
        cart,
        vid("mug"),
    ));
    assert_eq!(emptied, Ok(empty_in_croatia(cart, false)));
    assert_eq!(world.get(cart), Ok(empty_in_croatia(cart, false)));
}

#[test]
fn create_cart_rejects_country_without_market() {
    let world = World::new();
    let de = CountryCode::new("DE").unwrap();
    let result = world.create_cart_in("DE");
    assert_eq!(result, Err(CommandError::NoMarketForCountry(de)));
}

#[test]
fn create_cart_rejects_an_invalid_country() {
    let world = World::new();
    let result = world.create_cart_in("hr");
    assert_eq!(
        result,
        Err(CommandError::Country(CountryError::NotUppercase))
    );
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
            compare_at: None,
            price_list: Some(list_id()),
            subtotal: eur(2000),
            discounts: vec![],
            total: eur(2000),
            tax: None,
        }
    );
    assert_eq!(priced.total, eur(2000));
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
        Err(CommandError::Cart(CartError::Quantity(QuantityError::Zero)))
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
fn a_deleted_products_line_is_unavailable_until_it_is_removed() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();
    world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    world.delete_product("mug");
    let priced = world.get(cart).unwrap();
    assert_eq!(
        priced.lines[0].status,
        LineStatus::Unavailable(LineError::ProductNotFound(pid("mug")))
    );
    assert!(priced.lines[1].is_available());
    assert_eq!(priced.total, eur(1000));

    // Other lines still change, and the buyer can take it out.
    world.add(&Ctx::system(), cart, "cup", 1).unwrap();
    let priced = block_on(remove_cart_line(
        &Ctx::system(),
        &world.ports,
        cart,
        vid("mug"),
    ))
    .unwrap();
    assert_eq!(priced.lines.len(), 1);
    assert!(priced.is_fully_available());
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
fn a_line_is_priced_at_its_quantity() {
    let world = World::new();
    let cart = world.create_cart();
    let price = |a| Price::new(eur(a)).unwrap();
    let tiered = hypha_markets::FixedPrice::plain(price(450))
        .with_breaks(vec![hypha_markets::PriceBreak {
            min_quantity: 3,
            price: price(400),
        }])
        .unwrap();
    block_on(
        world
            .price_lists
            .set_fixed_prices(list_id(), &[(vid("cup"), tiered)]),
    )
    .unwrap();

    assert_eq!(
        world.add(&Ctx::system(), cart, "cup", 2).unwrap().total,
        eur(900)
    );
    // A third makes every one cheaper.
    assert_eq!(
        world.add(&Ctx::system(), cart, "cup", 1).unwrap().total,
        eur(1200)
    );
}

#[test]
fn uses_fixed_price_from_price_list() {
    let world = World::new();
    let cart = world.create_cart();
    let fixed = hypha_markets::FixedPrice::plain(Price::new(eur(450)).unwrap());
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

// ---- channels ----

#[test]
fn a_cart_needs_a_channel() {
    let world = World::new();
    let input = CreateCart {
        country: "HR".to_string(),
        idempotency: None,
        customer: None,
    };
    let result = block_on(create_cart(
        &Ctx::system(),
        &world.ports,
        &world.ids,
        &world.clock,
        input,
    ));
    assert_eq!(result.map(|_| ()), Err(CommandError::ChannelRequired));
}

#[test]
fn a_cart_belongs_to_the_callers_channel() {
    let world = World::new();
    let cart = world.create_cart();
    let stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(Some(stored.channel()), world.storefront().channel());
}

#[test]
fn another_channels_cart_reads_as_missing() {
    let world = World::new();
    let cart = world.create_cart();
    let elsewhere = Ctx::in_channel(
        Actor::System,
        Scope::ALL.to_vec(),
        hypha_core::ChannelId::from_uuid(Uuid::from_u128(999)),
    );
    assert_eq!(
        block_on(get_cart(&elsewhere, &world.ports, cart)).map(|_| ()),
        Err(CommandError::CartNotFound(cart))
    );
    assert_eq!(
        world.add(&elsewhere, cart, "mug", 1).map(|_| ()),
        Err(CommandError::CartNotFound(cart))
    );
    assert_eq!(world.stored_line_count(cart), 0);
    // Its own storefront and staff (in no channel) still see it.
    assert!(block_on(get_cart(&world.storefront(), &world.ports, cart)).is_ok());
    assert!(world.get(cart).is_ok());
}

#[test]
fn a_product_not_published_to_the_carts_channel_is_refused() {
    let world = World::new();
    let cart = world.create_cart();
    let channel = world.storefront().channel().unwrap();
    block_on(world.channels.unpublish(channel, &[pid("mug")])).unwrap();

    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 1).map(|_| ()),
        Err(CommandError::Line(LineError::ProductNotPublished(pid(
            "mug"
        ))))
    );
    assert_eq!(world.stored_line_count(cart), 0);
}

#[test]
fn an_unpublished_product_says_nothing_about_its_stock() {
    let world = World::new();
    world.stock("mug", 0);
    let cart = world.create_cart();
    let channel = world.storefront().channel().unwrap();
    block_on(world.channels.unpublish(channel, &[pid("mug")])).unwrap();
    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 1).map(|_| ()),
        Err(CommandError::Line(LineError::ProductNotPublished(pid(
            "mug"
        ))))
    );
}

#[test]
fn a_line_becomes_unavailable_when_its_product_is_unpublished() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();
    world.add(&Ctx::system(), cart, "cup", 1).unwrap();
    let channel = world.storefront().channel().unwrap();
    block_on(world.channels.unpublish(channel, &[pid("mug")])).unwrap();

    let priced = world.get(cart).unwrap();
    assert_eq!(
        priced.lines[0].status,
        LineStatus::Unavailable(LineError::ProductNotPublished(pid("mug")))
    );
    assert_eq!(priced.total, eur(1000), "only the cup");
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

// ---- stock ----

#[test]
fn add_line_refuses_more_than_the_market_has_and_stores_nothing() {
    let world = World::new();
    world.stock("mug", 2);
    let cart = world.create_cart();

    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 3),
        Err(CommandError::Line(LineError::OutOfStock {
            variant: vid("mug"),
            requested: 3,
            available: 2
        }))
    );
    assert_eq!(world.stored_line_count(cart), 0);
}

#[test]
fn add_line_checks_the_whole_line_not_just_the_added_units() {
    let world = World::new();
    world.stock("mug", 2);
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();

    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 1),
        Err(CommandError::Line(LineError::OutOfStock {
            variant: vid("mug"),
            requested: 3,
            available: 2
        }))
    );
}

#[test]
fn untracked_products_are_never_out_of_stock() {
    let world = World::new();
    world.stock("mug", 0);
    let cart = world.create_cart();

    // The cup has no inventory item.
    let priced = world.add(&Ctx::system(), cart, "cup", 500).unwrap();
    assert!(priced.is_fully_available());
}

#[test]
fn a_line_that_ran_out_after_adding_is_marked_and_kept() {
    let world = World::new();
    world.stock("mug", 3);
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();
    world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    world.sell_elsewhere("mug", 2);
    let priced = world.get(cart).unwrap();

    assert_eq!(
        priced.lines[0].status,
        LineStatus::Unavailable(LineError::OutOfStock {
            variant: vid("mug"),
            requested: 2,
            available: 1
        })
    );
    assert_eq!(priced.total, eur(1000), "only the cup counts");
    assert_eq!(
        world.stored_line_count(cart),
        2,
        "the line stays in the cart"
    );
}

#[test]
fn stock_at_a_location_serving_another_market_does_not_count() {
    let world = World::new();
    world.stock("mug", 0);
    let elsewhere = MarketId::from_uuid(Uuid::from_u128(1099));
    let mut far = Location::new(
        LocationId::from_uuid(Uuid::from_u128(1098)),
        "Belgrade".to_string(),
        vec![elsewhere],
        0,
    )
    .unwrap();
    block_on(world.locations.insert(&mut far)).unwrap();
    let mut item = block_on(world.inventory_items.get_by_variant(vid("mug")))
        .unwrap()
        .unwrap();
    item.set_on_hand(far.id(), 50, None).unwrap();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    let cart = world.create_cart();

    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 1),
        Err(CommandError::Line(LineError::OutOfStock {
            variant: vid("mug"),
            requested: 1,
            available: 0
        }))
    );
}

#[test]
fn continue_selling_allows_more_than_the_stock() {
    let world = World::new();
    world.stock("mug", 1);
    let mut item = block_on(world.inventory_items.get_by_variant(vid("mug")))
        .unwrap()
        .unwrap();
    item.set_continue_selling(true);
    block_on(world.inventory_items.update(&mut item)).unwrap();
    let cart = world.create_cart();

    assert!(world.add(&Ctx::system(), cart, "mug", 5).is_ok());
}

// ---- buyer ----

#[test]
fn set_cart_email_normalizes_and_clears() {
    let world = World::new();
    let cart = world.create_cart();

    let updated = block_on(set_cart_email(
        &Ctx::system(),
        &world.ports,
        cart,
        Some(" Ana@Example.hr".to_string()),
    ))
    .unwrap();
    assert_eq!(updated.email, Some(Email::new("ana@example.hr").unwrap()));
    assert_eq!(
        world.get(cart).unwrap().email,
        Some(Email::new("ana@example.hr").unwrap())
    );

    let cleared = block_on(set_cart_email(&Ctx::system(), &world.ports, cart, None)).unwrap();
    assert_eq!(cleared.email, None);
}

#[test]
fn set_cart_email_rejects_an_invalid_email() {
    let world = World::new();
    let cart = world.create_cart();
    assert_eq!(
        block_on(set_cart_email(
            &Ctx::system(),
            &world.ports,
            cart,
            Some("nope".to_string())
        )),
        Err(CommandError::Email(EmailError::Invalid("nope".to_string())))
    );
}

#[test]
fn set_cart_customer_links_the_customer_and_takes_their_email() {
    let world = World::new();
    let cart = world.create_cart();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    let ana = Email::new("ana@example.hr").unwrap();
    block_on(world.customers.insert(&mut Customer::new(id, ana.clone()))).unwrap();

    let updated = block_on(set_cart_customer(
        &Ctx::system(),
        &world.ports,
        cart,
        Some(id),
    ))
    .unwrap();
    assert_eq!(
        (updated.customer, updated.email),
        (Some(id), Some(ana.clone()))
    );

    let unlinked = block_on(set_cart_customer(&Ctx::system(), &world.ports, cart, None)).unwrap();
    assert_eq!((unlinked.customer, unlinked.email), (None, Some(ana)));
}

#[test]
fn set_cart_customer_rejects_an_unknown_customer() {
    let world = World::new();
    let cart = world.create_cart();
    let unknown = CustomerId::from_uuid(Uuid::from_u128(1599));
    assert_eq!(
        block_on(set_cart_customer(
            &Ctx::system(),
            &world.ports,
            cart,
            Some(unknown)
        )),
        Err(CommandError::CustomerNotFound(unknown))
    );
}

// ---- review fixes (docs/plans/2026-10-14-review-cleanup.md) ----

/// B1: a line whose total overflows used to be stored (and then shown as unavailable).
#[test]
fn add_line_refuses_a_line_whose_total_overflows_and_stores_nothing() {
    let world = World::new();
    let cart = world.create_cart();
    // +100 000 000 bp: a mug costs 1000 × 10 001 = 10 001 000 cents.
    world.set_adjustment(100_000_000);

    // 10 001 000 × 1 000 000 000 is above Money::MAX_AMOUNT.
    assert_eq!(
        world.add(&Ctx::system(), cart, "mug", 1_000_000_000),
        Err(CommandError::Line(LineError::Money(MoneyError::Overflow)))
    );
    assert_eq!(world.stored_line_count(cart), 0);
}

/// B2: lines that each fit can stop fitting together after a price change; the cart
/// must stay readable, with the line that doesn't fit marked unavailable.
#[test]
fn a_cart_whose_total_overflows_stays_readable() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 30_000_000).unwrap();
    world.add(&Ctx::system(), cart, "cup", 30_000_000).unwrap();

    // +2 000 000 000 bp: 200 001 000 cents each, so each line is 6.00003 × 10^15
    // (fits) but both together are 1.2 × 10^16 (doesn't).
    world.set_adjustment(2_000_000_000);
    let priced = world.get(cart).unwrap();

    assert!(priced.lines[0].is_available());
    assert_eq!(
        priced.lines[1].status,
        LineStatus::Unavailable(LineError::Money(MoneyError::Overflow))
    );
    assert_eq!(priced.total, eur(6_000_030_000_000_000));
}

/// B4: linking copies the customer's email onto the cart and reveals whether a
/// `cus_…` exists, so it needs `customers:read` too, checked before any lookup.
#[test]
fn set_cart_customer_requires_customers_read_to_link() {
    let world = World::new();
    let cart = world.create_cart();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    let ana = Email::new("ana@example.hr").unwrap();
    block_on(world.customers.insert(&mut Customer::new(id, ana))).unwrap();
    let cart_writer = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::CartsWrite],
    );
    let missing = Err(CommandError::Auth(AuthError::MissingScope(
        Scope::CustomersRead,
    )));

    assert_eq!(
        block_on(set_cart_customer(
            &cart_writer,
            &world.ports,
            cart,
            Some(id)
        )),
        missing
    );
    let unknown = CustomerId::from_uuid(Uuid::from_u128(1599));
    assert_eq!(
        block_on(set_cart_customer(
            &cart_writer,
            &world.ports,
            cart,
            Some(unknown)
        )),
        missing,
        "an unknown ID must not be distinguishable from a known one"
    );
    assert_eq!(world.get(cart).unwrap().email, None);
    // Unlinking reveals nothing and stays allowed.
    assert!(block_on(set_cart_customer(&cart_writer, &world.ports, cart, None)).is_ok());
}

#[test]
fn a_linked_cart_keeps_its_email_until_the_customer_is_unlinked() {
    let world = World::new();
    let cart = world.create_cart();
    let id = CustomerId::from_uuid(Uuid::from_u128(1501));
    let ana = Email::new("ana@example.hr").unwrap();
    block_on(world.customers.insert(&mut Customer::new(id, ana.clone()))).unwrap();
    block_on(set_cart_customer(
        &Ctx::system(),
        &world.ports,
        cart,
        Some(id),
    ))
    .unwrap();

    assert_eq!(
        block_on(set_cart_email(&Ctx::system(), &world.ports, cart, None)),
        Err(CommandError::Cart(CartError::EmailRequired))
    );
    assert_eq!(world.get(cart).unwrap().email, Some(ana));

    block_on(set_cart_customer(&Ctx::system(), &world.ports, cart, None)).unwrap();
    let cleared = block_on(set_cart_email(&Ctx::system(), &world.ports, cart, None)).unwrap();
    assert_eq!(cleared.email, None);
}

// ---- changing and removing lines ----

fn set_quantity(
    world: &World,
    cart: CartId,
    name: &str,
    quantity: u32,
) -> Result<PricedCart, CommandError> {
    block_on(set_line_quantity(
        &Ctx::system(),
        &world.ports,
        SetLineQuantity {
            cart,
            variant: vid(name),
            quantity,
        },
    ))
}

#[test]
fn set_line_quantity_replaces_the_quantity_and_reprices() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();

    let priced = set_quantity(&world, cart, "mug", 3).unwrap();

    assert_eq!(priced.lines[0].quantity.value(), 3);
    assert_eq!(priced.subtotal, eur(3000));
    assert_eq!(world.get(cart), Ok(priced));
}

#[test]
fn set_line_quantity_rejects_zero_and_a_missing_line() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();

    assert_eq!(
        set_quantity(&world, cart, "mug", 0),
        Err(CommandError::Cart(CartError::Quantity(QuantityError::Zero)))
    );
    assert_eq!(
        set_quantity(&world, cart, "cup", 2),
        Err(CommandError::Cart(CartError::LineNotFound(vid("cup"))))
    );
}

#[test]
fn raising_a_quantity_needs_stock_but_lowering_never_does() {
    let world = World::new();
    world.stock("mug", 3);
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 3).unwrap();
    world.sell_elsewhere("mug", 2);

    assert_eq!(
        set_quantity(&world, cart, "mug", 4),
        Err(CommandError::Line(LineError::OutOfStock {
            variant: vid("mug"),
            requested: 4,
            available: 1
        }))
    );
    assert_eq!(world.stored_quantity(cart, "mug"), 3);

    let lowered = set_quantity(&world, cart, "mug", 1).unwrap();
    assert!(lowered.is_fully_available());
}

#[test]
fn raising_the_quantity_of_an_archived_product_is_refused() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();
    world.archive("mug");

    assert_eq!(
        set_quantity(&world, cart, "mug", 2),
        Err(CommandError::Line(LineError::ProductNotActive(pid("mug"))))
    );
    assert!(set_quantity(&world, cart, "mug", 1).is_ok());
}

#[test]
fn remove_cart_line_takes_the_variant_out() {
    let world = World::new();
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();
    world.add(&Ctx::system(), cart, "cup", 1).unwrap();

    let priced = block_on(remove_cart_line(
        &Ctx::system(),
        &world.ports,
        cart,
        vid("mug"),
    ))
    .unwrap();

    assert_eq!(priced.lines.len(), 1);
    assert_eq!(world.stored_line_count(cart), 1);
    assert_eq!(
        block_on(remove_cart_line(
            &Ctx::system(),
            &world.ports,
            cart,
            vid("mug")
        )),
        Err(CommandError::Cart(CartError::LineNotFound(vid("mug"))))
    );
}

// ---------- idempotency keys ----------

fn keyed(key: &str, fingerprint: u8) -> Option<IdempotencyRequest> {
    Some(IdempotencyRequest {
        key: key.to_string(),
        fingerprint: [fingerprint; 32],
    })
}

impl World {
    fn create_keyed(
        &self,
        ctx: &Ctx,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<Idempotent<PricedCart>, CommandError> {
        let input = CreateCart {
            country: "HR".to_string(),
            idempotency,
            customer: None,
        };
        block_on(create_cart(ctx, &self.ports, &self.ids, &self.clock, input))
    }

    fn add_keyed(
        &self,
        cart: CartId,
        name: &str,
        quantity: u32,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<Idempotent<PricedCart>, CommandError> {
        let input = AddLine {
            cart,
            variant: vid(name),
            quantity,
            idempotency,
        };
        block_on(add_line(&Ctx::system(), &self.ports, &self.clock, input))
    }
}

#[test]
fn create_cart_retried_with_its_key_returns_the_first_cart() {
    let world = World::new();
    let first = world
        .create_keyed(&world.storefront(), keyed("k-1", 1))
        .unwrap();
    let retry = world
        .create_keyed(&world.storefront(), keyed("k-1", 1))
        .unwrap();
    assert!(!first.replayed);
    assert!(retry.replayed);
    assert_eq!(retry.value, first.value);
}

#[test]
fn add_line_retried_with_its_key_adds_once() {
    let world = World::new();
    let cart = world.create_cart();
    let first = world.add_keyed(cart, "mug", 2, keyed("k-1", 1)).unwrap();
    let retry = world.add_keyed(cart, "mug", 2, keyed("k-1", 1)).unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.value, first.value);
    assert_eq!(world.stored_quantity(cart, "mug"), 2);
}

#[test]
fn a_key_reused_for_a_different_request_is_refused() {
    let world = World::new();
    let cart = world.create_cart();
    world.add_keyed(cart, "mug", 2, keyed("k-1", 1)).unwrap();
    assert_eq!(
        world.add_keyed(cart, "mug", 3, keyed("k-1", 2)),
        Err(CommandError::Idempotency(IdempotencyError::KeyReused))
    );
    // The same fingerprint for another operation is a different request too.
    assert_eq!(
        world.create_keyed(&world.storefront(), keyed("k-1", 1)),
        Err(CommandError::Idempotency(IdempotencyError::KeyReused))
    );
    assert_eq!(world.stored_quantity(cart, "mug"), 2);
}

#[test]
fn an_invalid_key_is_refused_before_anything_is_written() {
    let world = World::new();
    let cart = world.create_cart();
    assert_eq!(
        world.add_keyed(cart, "mug", 1, keyed("two words", 1)),
        Err(CommandError::Idempotency(IdempotencyError::InvalidKey))
    );
    assert_eq!(world.stored_line_count(cart), 0);
}

#[test]
fn keys_belong_to_the_actor_that_sent_them() {
    let world = World::new();
    let user = Ctx::in_channel(
        Actor::User(UserId::from_uuid(Uuid::from_u128(7))),
        vec![Scope::CartsWrite],
        world.storefront().channel().unwrap(),
    );
    let system_cart = world
        .create_keyed(&world.storefront(), keyed("k-1", 1))
        .unwrap();
    let user_cart = world.create_keyed(&user, keyed("k-1", 1)).unwrap();
    assert!(!user_cart.replayed);
    assert_ne!(user_cart.value.id, system_cart.value.id);
}

#[test]
fn a_command_that_fails_records_no_key() {
    let world = World::new();
    world.stock("mug", 1);
    let cart = world.create_cart();
    assert!(world.add_keyed(cart, "mug", 2, keyed("k-1", 1)).is_err());

    let mut item = block_on(world.inventory_items.get_by_variant(vid("mug")))
        .unwrap()
        .unwrap();
    item.set_on_hand(warehouse(), 5, None).unwrap();
    block_on(world.inventory_items.update(&mut item)).unwrap();

    // Nothing was recorded, so the retry runs for real.
    let retry = world.add_keyed(cart, "mug", 2, keyed("k-1", 1)).unwrap();
    assert!(!retry.replayed);
    assert_eq!(world.stored_quantity(cart, "mug"), 2);
}

#[test]
fn losing_the_race_for_a_key_replays_the_winner() {
    let world = World::new();
    let winner = world
        .create_keyed(&world.storefront(), keyed("k-1", 1))
        .unwrap();
    // The retry's check runs before the winner's record is visible; its own write then
    // fails on the key, and nothing of it is stored.
    world.idempotency.hide_next_lookups(1);
    let loser = world
        .create_keyed(&world.storefront(), keyed("k-1", 1))
        .unwrap();
    assert!(loser.replayed);
    assert_eq!(loser.value.id, winner.value.id);
    let loser_cart = CartId::from_uuid(Uuid::from_u128(2));
    assert_eq!(block_on(world.carts.get(loser_cart)), Ok(None));
}

// ---------- a product's offer in a country ----------

fn product(world: &World, name: &str) -> Product {
    block_on(world.products.get(pid(name))).unwrap().unwrap()
}

fn shopper() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        vec![Scope::ProductsRead],
    )
}

#[test]
fn a_product_is_priced_and_stocked_in_the_countrys_market() {
    let world = World::new();
    world.set_adjustment(-1000);
    world.stock("mug", 1);
    let offer = block_on(hypha_cart::price_product(
        &shopper(),
        &world.ports,
        &product(&world, "mug"),
        "HR",
    ))
    .unwrap();
    assert_eq!(offer.market.as_ref().map(|m| m.id()), Some(market_id()));
    assert_eq!(
        offer.variants,
        vec![hypha_cart::VariantOffer {
            variant: vid("mug"),
            price: Some(hypha_pricing::ResolvedPrice {
                price: Price::new(eur(900)).unwrap(),
                compare_at: None,
                price_list: Some(list_id()),
                breaks: vec![],
            }),
            available: true,
            sellable: Some(1),
        }]
    );
    assert!(offer.sold());

    world.sell_elsewhere("mug", 1);
    let offer = block_on(hypha_cart::price_product(
        &shopper(),
        &world.ports,
        &product(&world, "mug"),
        "HR",
    ))
    .unwrap();
    assert!(!offer.variants[0].available, "sold out");
    assert!(offer.sold(), "still sold there, just not in stock");
}

#[test]
fn a_product_outside_the_catalog_or_country_is_not_sold_there() {
    let world = World::new();
    let poster = block_on(hypha_cart::price_product(
        &shopper(),
        &world.ports,
        &product(&world, "poster"),
        "HR",
    ))
    .unwrap();
    assert!(poster.market.is_some() && !poster.sold());
    assert_eq!(
        (
            poster.variants[0].price.clone(),
            poster.variants[0].available
        ),
        (None, false)
    );

    let elsewhere = block_on(hypha_cart::price_product(
        &shopper(),
        &world.ports,
        &product(&world, "mug"),
        "US",
    ))
    .unwrap();
    assert!(elsewhere.market.is_none() && !elsewhere.sold());

    assert_eq!(
        block_on(hypha_cart::price_product(
            &shopper(),
            &world.ports,
            &product(&world, "mug"),
            "hr"
        )),
        Err(CommandError::Country(CountryError::NotUppercase))
    );
    assert_eq!(
        block_on(hypha_cart::price_product(
            &Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), vec![]),
            &world.ports,
            &product(&world, "mug"),
            "HR"
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsRead
        )))
    );
}

// ---- discount codes ----

/// A live 10%-off-the-order discount by code `code`.
fn code_discount(world: &World, n: u128, code: &str, status: hypha_discounts::Status) {
    use hypha_discounts::{
        Discount, DiscountCode, DiscountDraft, DiscountRepo, Kind, Method, Reward, Target, Value,
    };
    let kind = Kind::Standard {
        rewards: vec![Reward {
            target: Target::Subtotal,
            value: Value::Percent(BasisPoints::new(1000).unwrap()),
        }],
    };
    let mut draft = DiscountDraft::new("Welcome", Method::Codes, kind);
    draft.status = status;
    let id = hypha_core::DiscountId::from_uuid(Uuid::from_u128(5000 + n));
    let mut discount = Discount::new(id, draft).unwrap();
    block_on(world.discounts.insert(&mut discount)).unwrap();
    block_on(
        world
            .discounts
            .add_codes(&[DiscountCode::new(id, code, None, None).unwrap()]),
    )
    .unwrap();
}

#[test]
fn a_live_code_is_held_and_applies() {
    let world = World::new();
    code_discount(&world, 1, "WELCOME10", hypha_discounts::Status::Active);
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 2).unwrap();
    let priced = block_on(add_discount_code(
        &Ctx::system(),
        &world.ports,
        cart,
        " welcome10",
    ))
    .unwrap();
    assert_eq!((priced.discount_total, priced.total), (eur(200), eur(1800)));
    assert_eq!(
        priced.discount_codes[0].state,
        hypha_cart::promotions::CodeState::Applied
    );
    // Stored normalised, so a read applies it too.
    assert_eq!(world.get(cart).unwrap().total, eur(1800));
}

#[test]
fn an_unknown_or_draft_code_is_refused_alike_and_not_kept() {
    let world = World::new();
    code_discount(&world, 1, "LATER", hypha_discounts::Status::Draft);
    let cart = world.create_cart();
    for code in ["NOPE", "later", "two words"] {
        assert_eq!(
            block_on(add_discount_code(&Ctx::system(), &world.ports, cart, code)),
            Err(CommandError::DiscountCodeInvalid)
        );
    }
    assert!(world.get(cart).unwrap().discount_codes.is_empty());
}

#[test]
fn a_code_held_by_an_empty_cart_waits_for_items() {
    let world = World::new();
    code_discount(&world, 1, "WELCOME10", hypha_discounts::Status::Active);
    let cart = world.create_cart();
    let priced = block_on(add_discount_code(
        &Ctx::system(),
        &world.ports,
        cart,
        "WELCOME10",
    ))
    .unwrap();
    assert_eq!(
        priced.discount_codes[0].state,
        hypha_cart::promotions::CodeState::NoMatchingItems
    );
}

#[test]
fn at_most_five_codes_are_held() {
    let world = World::new();
    let cart = world.create_cart();
    for n in 0..6u128 {
        code_discount(&world, n, &format!("C{n}"), hypha_discounts::Status::Active);
    }
    for n in 0..5 {
        block_on(add_discount_code(
            &Ctx::system(),
            &world.ports,
            cart,
            &format!("C{n}"),
        ))
        .unwrap();
    }
    assert_eq!(
        block_on(add_discount_code(&Ctx::system(), &world.ports, cart, "C5")),
        Err(CommandError::Cart(CartError::TooManyDiscountCodes {
            max: 5
        }))
    );
}

#[test]
fn a_code_is_let_go_by_its_text_however_typed() {
    let world = World::new();
    code_discount(&world, 1, "WELCOME10", hypha_discounts::Status::Active);
    let cart = world.create_cart();
    world.add(&Ctx::system(), cart, "mug", 1).unwrap();
    block_on(add_discount_code(
        &Ctx::system(),
        &world.ports,
        cart,
        "WELCOME10",
    ))
    .unwrap();
    let priced = block_on(remove_discount_code(
        &Ctx::system(),
        &world.ports,
        cart,
        "welcome10",
    ))
    .unwrap();
    assert_eq!(
        (priced.discount_total, priced.discount_codes.len()),
        (eur(0), 0)
    );
    assert_eq!(
        block_on(remove_discount_code(
            &Ctx::system(),
            &world.ports,
            cart,
            "WELCOME10"
        )),
        Err(CommandError::Cart(CartError::DiscountCodeNotOnCart(
            "WELCOME10".to_string()
        )))
    );
}
