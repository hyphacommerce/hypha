use hypha_cart::{
    AddLine, CartError, CartRepo, CommandError, LineError, LineStatus, PricedCart, SetLineQuantity,
    add_line, create_cart, get_cart, remove_cart_line, set_cart_customer, set_cart_email,
    set_line_quantity,
};
use hypha_core::{
    Actor, AuthError, BasisPoints, CartId, CatalogId, CountryCode, CountryError, Ctx, Currency,
    InventoryItemId, LocationId, MarketId, Money, Price, PriceListId, ProductId, Scope, TaxClassId,
    TaxRate, TaxRegionId, UserId, VariantId,
};
use hypha_core::{CustomerId, Email, EmailError, MoneyError, Quantity, QuantityError};
use hypha_customers::{Customer, CustomerRepo};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_pricing::PricingError;
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_tax::{TaxClass, TaxClassRepo, TaxLine, TaxRegion, TaxRegionRepo, TaxSource};
use hypha_testkit::{InMemoryPorts, SequentialIds};
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
    Money::new(amount, Currency::Eur).unwrap()
}

/// The repositories (`InMemoryPorts`, reached through `Deref`) and an ID generator.
struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
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
        block_on(create_cart(&Ctx::system(), &self.ports, &self.ids, "HR"))
            .unwrap()
            .id
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
            &self.ports,
            AddLine {
                cart,
                variant: vid(name),
                quantity,
            },
        ))
    }

    fn get(&self, cart: CartId) -> Result<PricedCart, CommandError> {
        block_on(get_cart(&Ctx::system(), &self.ports, cart))
    }

    fn archive(&self, name: &str) {
        let mut product = block_on(self.products.get(pid(name))).unwrap().unwrap();
        product.set_status(ProductStatus::Archived).unwrap();
        block_on(self.products.update(&mut product)).unwrap();
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
    let cart = block_on(create_cart(&Ctx::system(), &world.ports, &world.ids, "HR")).unwrap();
    assert_eq!((cart.country, cart.lines.len()), (hr(), 0));
    let stored = block_on(world.carts.get(cart.id)).unwrap().unwrap();
    assert_eq!((stored.market(), stored.country()), (market_id(), hr()));
    assert_eq!(world.get(cart.id), Ok(cart));
}

#[test]
fn create_cart_rejects_country_without_market() {
    let world = World::new();
    let de = CountryCode::new("DE").unwrap();
    let result = block_on(create_cart(&Ctx::system(), &world.ports, &world.ids, "DE"));
    assert_eq!(result, Err(CommandError::NoMarketForCountry(de)));
}

#[test]
fn create_cart_rejects_an_invalid_country() {
    let world = World::new();
    let result = block_on(create_cart(&Ctx::system(), &world.ports, &world.ids, "hr"));
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
