use hypha_cart::{
    AddLine, CartRepo, CartStatus, CreateCart, add_line, create_cart, set_cart_customer,
    set_cart_email,
};
use hypha_checkout::{Checkout, CommandError, checkout, view_cart};
use hypha_core::{
    Actor, AuthError, CartId, Clock, CountryCode, Ctx, Currency, CustomerId, Email,
    IdempotencyRequest, Idempotent, InventoryItemId, LocationId, MarketId, Money, Price, ProductId,
    Quantity, Scope, TaxRate, TaxRegionId, UserId, VariantId,
};
use hypha_customers::{Customer, CustomerRepo};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo, Market, MarketRepo};
use hypha_orders::{FinancialStatus, Order, OrderNumber, OrderRepo, OrderTaxSource};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_tax::{TaxRegion, TaxRegionRepo};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

// IDs from 1000 up, so they never collide with SequentialIds (which counts from 1).
fn market_id() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1000))
}

fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(1001))
}

fn region() -> TaxRegionId {
    TaxRegionId::from_uuid(Uuid::from_u128(1002))
}

fn pid(name: &str) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(if name == "mug" { 1100 } else { 1101 }))
}

fn vid(name: &str) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(if name == "mug" { 1200 } else { 1201 }))
}

fn hr() -> CountryCode {
    CountryCode::new("HR").unwrap()
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::Eur).unwrap()
}

fn ana() -> Email {
    Email::new("ana@example.hr").unwrap()
}

/// The ports, an ID generator and a clock.
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
    /// An EUR market for Croatia (prices exclude tax; 25% VAT) whose catalog holds an
    /// active mug (10.00, 5 in stock at the warehouse) and cup (4.00, untracked).
    fn new() -> Self {
        let world = Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
        };
        let mut market = Market::new(
            market_id(),
            "Croatia".to_string(),
            vec![hr()],
            Currency::Eur,
        )
        .unwrap();
        block_on(world.markets.insert(&mut market)).unwrap();

        for (name, price) in [("mug", 1000), ("cup", 400)] {
            let mut product = Product::new_simple(
                pid(name),
                Handle::new(name).unwrap(),
                name.to_uppercase(),
                vid(name),
                Price::new(eur(price)).unwrap(),
            )
            .unwrap();
            product.set_status(ProductStatus::Active).unwrap();
            block_on(world.products.insert(&mut product)).unwrap();
        }
        let catalog_id = hypha_core::CatalogId::from_uuid(Uuid::from_u128(1003));
        let mut catalog = Catalog::new(catalog_id, CatalogContext::Market(market_id()), None);
        block_on(world.catalogs.insert(&mut catalog)).unwrap();
        block_on(
            world
                .catalogs
                .add_products(catalog_id, &[pid("mug"), pid("cup")]),
        )
        .unwrap();

        let mut location =
            Location::new(warehouse(), "Zagreb".to_string(), vec![market_id()], 0).unwrap();
        block_on(world.locations.insert(&mut location)).unwrap();
        let mut item = InventoryItem::new(
            InventoryItemId::from_uuid(Uuid::from_u128(1004)),
            vid("mug"),
            true,
            false,
        );
        item.set_on_hand(warehouse(), 5, None).unwrap();
        block_on(world.inventory_items.insert(&mut item)).unwrap();

        let mut vat = TaxRegion::new(region(), hr(), TaxRate::new(250_000).unwrap());
        block_on(world.tax_regions.insert(&mut vat)).unwrap();
        world
    }

    /// A cart for `ana@example.hr` holding 2 mugs and 1 cup: 24.00 + 6.00 VAT.
    fn cart(&self) -> CartId {
        let ctx = Ctx::system();
        let input = CreateCart {
            country: "HR".to_string(),
            idempotency: None,
        };
        let cart = block_on(create_cart(
            &ctx,
            &self.ports,
            &self.ids,
            &self.clock,
            input,
        ))
        .unwrap()
        .value
        .id;
        for (name, quantity) in [("mug", 2), ("cup", 1)] {
            self.add(cart, name, quantity);
        }
        block_on(set_cart_email(
            &ctx,
            &self.ports,
            cart,
            Some(ana().to_string()),
        ))
        .unwrap();
        cart
    }

    fn add(&self, cart: CartId, name: &str, quantity: u32) {
        let input = AddLine {
            cart,
            variant: vid(name),
            quantity,
            idempotency: None,
        };
        block_on(add_line(&Ctx::system(), &self.ports, &self.clock, input)).unwrap();
    }

    fn checkout_as(
        &self,
        ctx: &Ctx,
        cart: CartId,
        expected_total: Option<Money>,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<Idempotent<Order>, CommandError> {
        let input = Checkout {
            cart,
            expected_total,
            idempotency,
        };
        block_on(checkout(ctx, &self.ports, &self.ids, &self.clock, input))
    }

    fn checkout(&self, cart: CartId) -> Result<Order, CommandError> {
        self.checkout_as(&Ctx::system(), cart, None, None)
            .map(|placed| placed.value)
    }

    fn mug_reserved(&self) -> i64 {
        block_on(self.inventory_items.get_by_variant(vid("mug")))
            .unwrap()
            .unwrap()
            .reserved(warehouse())
    }
}

#[test]
fn checkout_places_an_order_from_the_priced_cart() {
    let world = World::new();
    let cart = world.cart();
    let order = world.checkout(cart).unwrap();

    assert_eq!(order.number(), OrderNumber::new(1001).unwrap());
    assert_eq!(order.placed_at(), world.clock.now());
    assert_eq!(order.financial_status(), FinancialStatus::Pending);
    assert_eq!((order.cart(), order.market()), (cart, market_id()));
    assert_eq!(order.email(), &ana());
    assert_eq!(
        (order.subtotal(), order.tax_total(), order.total()),
        (eur(2400), eur(600), eur(3000))
    );

    let mug = &order.lines()[0];
    assert_eq!(mug.product_title, "MUG");
    assert_eq!(mug.quantity, Quantity::new(2).unwrap());
    assert_eq!(mug.unit_price, Price::new(eur(1000)).unwrap());
    let tax = mug.tax.unwrap();
    assert_eq!(tax.amount, eur(500));
    assert_eq!(tax.source, OrderTaxSource::RateTable { region: region() });

    assert_eq!(block_on(world.orders.get(order.id())), Ok(Some(order)));
}

#[test]
fn checkout_completes_the_cart() {
    let world = World::new();
    let cart = world.cart();
    let order = world.checkout(cart).unwrap();
    let stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(stored.status(), CartStatus::Completed { order: order.id() });
}

#[test]
fn checkout_reserves_tracked_stock_under_the_order_line() {
    let world = World::new();
    let order = world.checkout(world.cart()).unwrap();
    assert_eq!(world.mug_reserved(), 2);

    let item = block_on(world.inventory_items.get_by_variant(vid("mug")))
        .unwrap()
        .unwrap();
    let reference = ReservationRef::new(&order.lines()[0].id.to_string()).unwrap();
    assert!(item.reservations().iter().any(|r| r.reference == reference));
}

#[test]
fn a_guest_becomes_a_customer() {
    let world = World::new();
    let order = world.checkout(world.cart()).unwrap();
    let customer = block_on(world.customers.get_by_email(&ana()))
        .unwrap()
        .unwrap();
    assert_eq!(order.customer(), customer.id());
}

#[test]
fn a_customer_with_the_email_is_reused() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    block_on(world.customers.insert(&mut Customer::new(id, ana()))).unwrap();
    let order = world.checkout(world.cart()).unwrap();
    assert_eq!(order.customer(), id);
}

#[test]
fn the_linked_customer_is_the_buyer() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    let marko = Email::new("marko@example.hr").unwrap();
    block_on(
        world
            .customers
            .insert(&mut Customer::new(id, marko.clone())),
    )
    .unwrap();
    let cart = world.cart();
    block_on(set_cart_customer(
        &Ctx::system(),
        &world.ports,
        cart,
        Some(id),
    ))
    .unwrap();

    let order = world.checkout(cart).unwrap();
    assert_eq!((order.customer(), order.email()), (id, &marko));
    // No customer was made from the cart's earlier email.
    assert_eq!(block_on(world.customers.get_by_email(&ana())), Ok(None));
}

#[test]
fn a_customer_created_meanwhile_is_found_on_the_retry() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    block_on(world.customers.insert(&mut Customer::new(id, ana()))).unwrap();
    // The first attempt doesn't see the customer and stages a new one, which fails on
    // the email key; the retry finds the customer.
    world.customers.hide_next_lookups(1);
    let order = world.checkout(world.cart()).unwrap();
    assert_eq!(order.customer(), id);
    assert_eq!(world.mug_reserved(), 2);
}

#[test]
fn a_checked_out_cart_reports_its_order() {
    let world = World::new();
    let cart = world.cart();
    let order = world.checkout(cart).unwrap();
    assert_eq!(
        world.checkout(cart),
        Err(CommandError::CartCompleted {
            cart,
            order: order.id()
        })
    );
    assert_eq!(world.mug_reserved(), 2);
}

#[test]
fn retried_with_its_key_checkout_replays_the_order() {
    let world = World::new();
    let cart = world.cart();
    let key = || {
        Some(IdempotencyRequest {
            key: "co-1".to_string(),
            fingerprint: [1; 32],
        })
    };
    let first = world
        .checkout_as(&Ctx::system(), cart, None, key())
        .unwrap();
    let retry = world
        .checkout_as(&Ctx::system(), cart, None, key())
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.value, first.value);
}

#[test]
fn a_retry_that_loses_the_race_to_its_own_key_replays_the_order() {
    let world = World::new();
    let cart = world.cart();
    let key = || {
        Some(IdempotencyRequest {
            key: "co-1".to_string(),
            fingerprint: [1; 32],
        })
    };
    let first = world
        .checkout_as(&Ctx::system(), cart, None, key())
        .unwrap();
    // The retry's first check runs before the first request's record is visible, so it
    // goes on and finds the cart completed: that is the first request's order.
    world.idempotency.hide_next_lookups(1);
    let retry = world
        .checkout_as(&Ctx::system(), cart, None, key())
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.value, first.value);
    assert_eq!(world.mug_reserved(), 2);
}

#[test]
fn an_empty_cart_is_refused() {
    let world = World::new();
    let input = CreateCart {
        country: "HR".to_string(),
        idempotency: None,
    };
    let cart = block_on(create_cart(
        &Ctx::system(),
        &world.ports,
        &world.ids,
        &world.clock,
        input,
    ))
    .unwrap()
    .value
    .id;
    assert_eq!(world.checkout(cart), Err(CommandError::EmptyCart(cart)));
}

#[test]
fn a_cart_without_an_email_is_refused() {
    let world = World::new();
    let cart = world.cart();
    block_on(set_cart_email(&Ctx::system(), &world.ports, cart, None)).unwrap();
    assert_eq!(world.checkout(cart), Err(CommandError::MissingEmail(cart)));
}

#[test]
fn a_line_that_went_stale_is_refused_and_nothing_is_written() {
    let world = World::new();
    let cart = world.cart();
    let mut mug = block_on(world.products.get(pid("mug"))).unwrap().unwrap();
    mug.set_status(ProductStatus::Archived).unwrap();
    block_on(world.products.update(&mut mug)).unwrap();

    assert_eq!(
        world.checkout(cart),
        Err(CommandError::Unavailable {
            lines: vec![vid("mug")]
        })
    );
    let stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(stored.status(), CartStatus::Open);
    assert_eq!(world.mug_reserved(), 0);
}

#[test]
fn a_changed_total_is_refused() {
    let world = World::new();
    let cart = world.cart();
    assert_eq!(
        world
            .checkout_as(&Ctx::system(), cart, Some(eur(2900)), None)
            .map(|placed| placed.value),
        Err(CommandError::TotalChanged {
            expected: eur(2900),
            actual: eur(3000)
        })
    );
    assert!(
        world
            .checkout_as(&Ctx::system(), cart, Some(eur(3000)), None)
            .is_ok()
    );
}

#[test]
fn checkout_needs_the_checkout_scope() {
    let world = World::new();
    let cart = world.cart();
    let shop = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(7))),
        vec![Scope::CartsWrite, Scope::OrdersWrite],
    );
    assert_eq!(
        world.checkout_as(&shop, cart, None, None),
        Err(CommandError::Auth(AuthError::MissingScope(Scope::Checkout)))
    );
}

#[test]
fn a_completed_cart_refuses_new_lines() {
    let world = World::new();
    let cart = world.cart();
    let order = world.checkout(cart).unwrap();
    let input = AddLine {
        cart,
        variant: vid("cup"),
        quantity: 1,
        idempotency: None,
    };
    assert_eq!(
        block_on(add_line(&Ctx::system(), &world.ports, &world.clock, input)),
        Err(hypha_cart::CommandError::Cart(
            hypha_cart::CartError::Completed { order: order.id() }
        ))
    );
}

#[test]
fn long_option_labels_and_skus_are_copied_whole() {
    use hypha_core::{OptionId, OptionValueId};
    use hypha_products::{NewVariant, OptionValue, ProductOption};

    let world = World::new();
    // Three options with 100-character labels: the joined variant title is longer than
    // any single text field, and the SKU is too. Products allow both, so checkout must.
    let options: Vec<_> = ["a", "b", "c"]
        .iter()
        .enumerate()
        .map(|(n, letter)| {
            let n = n as u128;
            ProductOption::new(
                OptionId::from_uuid(Uuid::from_u128(1300 + n)),
                format!("Option {letter}"),
                vec![OptionValue::new(
                    OptionValueId::from_uuid(Uuid::from_u128(1310 + n)),
                    letter.repeat(100),
                )],
            )
        })
        .collect();
    let poster = ProductId::from_uuid(Uuid::from_u128(1320));
    let variant = VariantId::from_uuid(Uuid::from_u128(1321));
    let sku = "S".repeat(300);
    let mut product = Product::new_configurable(
        poster,
        Handle::new("poster").unwrap(),
        "Poster".to_string(),
        options,
        NewVariant {
            id: variant,
            sku: Some(sku.clone()),
            price: Price::new(eur(500)).unwrap(),
            selections: (0..3)
                .map(|n| OptionValueId::from_uuid(Uuid::from_u128(1310 + n)))
                .collect(),
        },
    )
    .unwrap();
    product.set_status(ProductStatus::Active).unwrap();
    block_on(world.products.insert(&mut product)).unwrap();
    let catalog = hypha_core::CatalogId::from_uuid(Uuid::from_u128(1003));
    block_on(world.catalogs.add_products(catalog, &[poster])).unwrap();

    let cart = world.cart();
    let input = AddLine {
        cart,
        variant,
        quantity: 1,
        idempotency: None,
    };
    block_on(add_line(&Ctx::system(), &world.ports, &world.clock, input)).unwrap();

    let order = world.checkout(cart).unwrap();
    let line = order.lines().iter().find(|l| l.variant == variant).unwrap();
    let title = ["a", "b", "c"].map(|l| l.repeat(100)).join(" / ");
    assert_eq!(line.variant_title.as_deref(), Some(title.as_str()));
    assert_eq!(line.sku.as_deref(), Some(sku.as_str()));
}

#[test]
fn a_checked_out_cart_reads_as_its_order_charged_it() {
    let world = World::new();
    let cart = world.cart();
    world.add(cart, "mug", 3); // all 5 mugs
    let order = world.checkout(cart).unwrap();

    // Priced now, the cart's mugs are out of stock: its own order reserved them.
    let now = block_on(hypha_cart::get_cart(&Ctx::system(), &world.ports, cart)).unwrap();
    assert!(!now.is_fully_available());

    let view = block_on(view_cart(&Ctx::system(), &world.ports, cart)).unwrap();
    assert_eq!(view.order, Some(order.id()));
    assert!(view.is_fully_available());
    assert_eq!(
        (view.subtotal, view.tax_total, view.total),
        (order.subtotal(), order.tax_total(), order.total())
    );
    assert_eq!(view.lines.len(), order.lines().len());
    assert_eq!(view.email.as_ref(), Some(&ana()));
}

#[test]
fn an_open_cart_reads_at_current_prices() {
    let world = World::new();
    let cart = world.cart();
    let view = block_on(view_cart(&Ctx::system(), &world.ports, cart)).unwrap();
    let now = block_on(hypha_cart::get_cart(&Ctx::system(), &world.ports, cart)).unwrap();
    assert_eq!(view, now);
}

#[test]
fn a_linked_customer_who_changed_email_gets_the_order_at_the_new_one() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    let mut marko = Customer::new(id, Email::new("marko@example.hr").unwrap());
    block_on(world.customers.insert(&mut marko)).unwrap();
    let cart = world.cart();
    block_on(set_cart_customer(
        &Ctx::system(),
        &world.ports,
        cart,
        Some(id),
    ))
    .unwrap();
    // After linking, the customer changes their email; the cart's copy is now stale.
    let new_email = Email::new("marko@novi.hr").unwrap();
    marko.set_email(new_email.clone());
    block_on(world.customers.update(&mut marko)).unwrap();

    let order = world.checkout(cart).unwrap();
    assert_eq!((order.customer(), order.email()), (id, &new_email));
}

#[test]
fn a_completed_cart_is_reported_before_other_checks() {
    let world = World::new();
    let cart = world.cart();
    let order = world.checkout(cart).unwrap();
    let completed = Err(hypha_cart::CommandError::Cart(
        hypha_cart::CartError::Completed { order: order.id() },
    ));
    let unknown = VariantId::from_uuid(Uuid::from_u128(1999));

    let add = AddLine {
        cart,
        variant: unknown,
        quantity: 1,
        idempotency: None,
    };
    assert_eq!(
        block_on(add_line(&Ctx::system(), &world.ports, &world.clock, add)).map(|_| ()),
        completed
    );
    let set = hypha_cart::SetLineQuantity {
        cart,
        variant: unknown,
        quantity: 1,
    };
    assert_eq!(
        block_on(hypha_cart::set_line_quantity(
            &Ctx::system(),
            &world.ports,
            set
        ))
        .map(|_| ()),
        completed
    );
    let nobody = CustomerId::from_uuid(Uuid::from_u128(1999));
    assert_eq!(
        block_on(set_cart_customer(
            &Ctx::system(),
            &world.ports,
            cart,
            Some(nobody)
        ))
        .map(|_| ()),
        completed
    );
}
