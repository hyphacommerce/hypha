#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]

use hypha_cart::{
    AddLine, CartRepo, CartStatus, CreateCart, add_line, create_cart, set_cart_customer,
    set_cart_email,
};
use hypha_checkout::{
    CartPaymentStarted, Checkout, CommandError, Placed, StartCartPayment, cart_payment_methods,
    checkout, complete_paid_checkout, set_payment_method, start_cart_payment, view_cart,
};
use hypha_core::{
    Actor, AuthError, CartId, Clock, CountryCode, Ctx, Currency, CustomerId, Email,
    IdempotencyRequest, Idempotent, InventoryItemId, LocationId, MarketId, Money, Price, ProductId,
    Quantity, Scope, TaxRate, TaxRegionId, UserId, VariantId,
};
use hypha_core::{PaymentMethodId, Versioned};
use hypha_customers::{Customer, CustomerRepo};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_markets::{Catalog, CatalogContext, CatalogRepo, Market, MarketRepo};
use hypha_orders::{FinancialStatus, Order, OrderNumber, OrderRepo, OrderTaxSource};
use hypha_payments::{
    ManualProvider, NewPaymentMethod, NextAction, PaymentAction, PaymentMethod, PaymentMethodRepo,
    PaymentRepo, PaymentStatus, ProviderKey, ProviderState,
};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus};
use hypha_tax::{TaxRegion, TaxRegionRepo};
use hypha_testkit::{FixedClock, InMemoryPorts, NoRateProviders, SequentialIds};
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
    Money::new(amount, Currency::EUR).unwrap()
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
            Currency::EUR,
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
            not_shipped(&mut product, vid(name));
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
        let ctx = self.ports.storefront();
        let input = CreateCart {
            country: "HR".to_string(),
            idempotency: None,
            customer: None,
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
        self.ports.pay_by_hand(cart);
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
        self.place_as(ctx, cart, expected_total, idempotency)
            .map(|placed| Idempotent {
                value: placed.value.order,
                replayed: placed.replayed,
            })
    }

    fn place_as(
        &self,
        ctx: &Ctx,
        cart: CartId,
        expected_total: Option<Money>,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<Idempotent<Placed>, CommandError> {
        let input = Checkout {
            cart,
            expected_total,
            idempotency,
            return_url: None,
        };
        block_on(checkout(
            ctx,
            &self.ports,
            &self.ids,
            &self.clock,
            &NoRateProviders,
            &(ManualProvider,),
            input,
        ))
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
    assert_eq!(Some(order.channel()), world.ports.storefront().channel());
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
fn checkout_records_the_order_and_the_new_customer_with_them() {
    let world = World::new();
    let before = world.ports.outbox.events().len();
    let order = world.checkout(world.cart()).unwrap();
    let events: Vec<_> = world.ports.outbox.events().split_off(before);
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert!(types.contains(&"order.placed"), "{types:?}");
    assert!(types.contains(&"customer.created"), "{types:?}");
    let placed = events
        .iter()
        .find(|e| e.event_type == "order.placed")
        .unwrap();
    assert_eq!(placed.data["order"], order.id().to_string());
    assert_eq!(placed.data["number"], order.number().value());
    assert_eq!(placed.data["customer"], order.customer().to_string());
    assert_eq!(placed.data["channel"], order.channel().to_string());
}

#[test]
fn a_customer_with_the_email_is_reused() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    block_on(world.customers.insert(&mut Customer::new(id, ana()))).unwrap();
    let order = world.checkout(world.cart()).unwrap();
    assert_eq!(order.customer(), id);
}

/// Ticks the cart's marketing box.
fn tick_marketing(world: &World, cart: CartId) {
    block_on(hypha_cart::set_cart_marketing(
        &Ctx::system(),
        &world.ports,
        cart,
        true,
    ))
    .unwrap();
}

fn email_consent(world: &World, id: CustomerId) -> hypha_customers::MarketingConsent {
    block_on(world.customers.get(id))
        .unwrap()
        .unwrap()
        .marketing_consent(hypha_customers::Channel::Email)
}

#[test]
fn ticking_the_box_subscribes_a_known_customer() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    block_on(world.customers.insert(&mut Customer::new(id, ana()))).unwrap();
    let cart = world.cart();
    tick_marketing(&world, cart);
    world.checkout(cart).unwrap();
    let consent = email_consent(&world, id);
    assert_eq!(
        (consent.state, consent.source),
        (
            hypha_customers::ConsentState::Subscribed,
            Some(hypha_customers::ConsentSource::Checkout)
        )
    );
}

#[test]
fn an_erased_customer_ticking_the_box_orders_but_isnt_subscribed() {
    let world = World::new();
    let id = CustomerId::from_uuid(Uuid::from_u128(1500));
    block_on(world.customers.insert(&mut Customer::new(id, ana()))).unwrap();
    let cart = world.cart();
    block_on(set_cart_customer(
        &Ctx::system(),
        &world.ports,
        cart,
        Some(id),
    ))
    .unwrap();
    let mut customer = block_on(world.customers.get(id)).unwrap().unwrap();
    customer.erase(world.clock.now());
    block_on(world.customers.update(&mut customer)).unwrap();
    tick_marketing(&world, cart);
    let before = email_consent(&world, id);
    assert_eq!(world.checkout(cart).map(|o| o.customer()), Ok(id));
    assert_eq!(email_consent(&world, id), before);
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
fn a_checked_out_cart_answers_with_its_order() {
    let world = World::new();
    let cart = world.cart();
    let order = world.checkout(cart).unwrap();
    // The shopper's browser and the provider's webhook may both complete a cart: the second
    // gets the order, and nothing is reserved twice.
    let again = world.checkout_as(&Ctx::system(), cart, None, None).unwrap();
    assert_eq!(again.value.id(), order.id());
    assert!(again.replayed);
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
fn another_channels_cart_cant_be_checked_out() {
    let world = World::new();
    let cart = world.cart();
    let elsewhere = Ctx::in_channel(
        Actor::System,
        Scope::ALL.to_vec(),
        hypha_core::ChannelId::from_uuid(Uuid::from_u128(999)),
    );
    assert_eq!(
        world.checkout_as(&elsewhere, cart, None, None).map(|_| ()),
        Err(CommandError::CartNotFound(cart))
    );
    assert_eq!(
        block_on(view_cart(&elsewhere, &world.ports, cart)).map(|_| ()),
        Err(CommandError::CartNotFound(cart))
    );
    assert!(
        world
            .checkout_as(&world.ports.storefront(), cart, None, None)
            .is_ok()
    );
}

#[test]
fn an_empty_cart_is_refused() {
    let world = World::new();
    let input = CreateCart {
        country: "HR".to_string(),
        idempotency: None,
        customer: None,
    };
    let cart = block_on(create_cart(
        &world.ports.storefront(),
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
fn a_deleted_products_line_is_refused_until_it_is_removed() {
    use hypha_core::HasBatch;
    use hypha_products::StageProducts;
    let world = World::new();
    let cart = world.cart();
    let mut mug = block_on(world.products.get(pid("mug"))).unwrap().unwrap();
    mug.delete();
    let mut batch = world.ports.batch();
    batch.delete_product(&mut mug);
    block_on(world.ports.commit(batch)).unwrap();

    assert_eq!(
        world.checkout(cart),
        Err(CommandError::Unavailable {
            lines: vec![vid("mug")]
        })
    );
    let stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(stored.status(), CartStatus::Open);
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
fn long_option_labels_and_skus_are_copied_whole() {
    use hypha_core::{OptionId, OptionValueId};
    use hypha_products::{NewVariant, OptionValue, ProductOption};

    let world = World::new();
    // Three options with 100-character labels: the joined variant title is longer than
    // any single text field. The SKU is as long as products allow (the text rule, 255
    // characters). Checkout must copy both whole.
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
    let sku = "S".repeat(hypha_core::MAX_TEXT_LEN);
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
    not_shipped(&mut product, variant);
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
    marko.set_email(new_email.clone()).unwrap();
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

/// These tests are about stock, tax, buyers and retries: their products aren't shipped,
/// so checkout needs no address or option (shipping has its own tests, `shipping.rs`).
fn not_shipped(product: &mut Product, variant: VariantId) {
    product
        .update_variant(
            variant,
            hypha_products::VariantPatch {
                requires_shipping: Some(false),
                ..hypha_products::VariantPatch::default()
            },
        )
        .unwrap();
}

// ---- payments ----

fn bank_transfer() -> hypha_core::PaymentMethodId {
    hypha_core::PaymentMethodId::from_uuid(Uuid::from_u128(InMemoryPorts::BANK_TRANSFER))
}

/// A cart like `cart`, without a payment method chosen.
fn cart_without_method(world: &World) -> CartId {
    let cart = world.cart();
    let mut stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    stored.set_payment_method(None).unwrap();
    block_on(world.carts.update(&mut stored)).unwrap();
    cart
}

/// A second manual method, "Cash on delivery", offered where `markets` say.
fn add_cod(world: &World, n: u128, markets: Vec<MarketId>, position: u32) -> PaymentMethodId {
    let mut method = PaymentMethod::new(NewPaymentMethod {
        id: PaymentMethodId::from_uuid(Uuid::from_u128(n)),
        provider: ProviderKey::manual(),
        name: "Cash on delivery".to_string(),
        instructions: None,
        markets,
        channels: vec![],
        enabled: true,
        position,
        settings: serde_json::json!({}),
    })
    .unwrap();
    block_on(world.payment_methods.insert(&mut method)).unwrap();
    method.id()
}

#[test]
fn checkout_writes_a_pending_payment_and_answers_with_the_instructions() {
    let world = World::new();
    let cart = world.cart();
    let placed = world
        .place_as(&Ctx::system(), cart, None, None)
        .unwrap()
        .value;
    let payment = placed.payment.clone().unwrap();
    assert_eq!(payment.order(), Some(placed.order.id()));
    assert_eq!(payment.cart(), Some(cart));
    assert_eq!(payment.method(), Some(bank_transfer()));
    assert_eq!(payment.method_name(), Some("Bank transfer"));
    assert_eq!(payment.status(), PaymentStatus::Pending);
    assert_eq!(payment.amount(), placed.order.total());
    assert_eq!(
        payment.instructions(),
        Some("IBAN HR12 3456 7890; quote your order number")
    );
    assert_eq!(
        placed.next_action,
        Some(Ok(NextAction::Instructions(
            "IBAN HR12 3456 7890; quote your order number".to_string()
        )))
    );
    assert_eq!(
        block_on(world.payments.list_for_order(placed.order.id())),
        Ok(vec![payment])
    );
    assert_eq!(placed.order.financial_status(), FinancialStatus::Pending);
    assert!(
        world
            .outbox
            .types()
            .contains(&"payment.created".to_string())
    );
}

#[test]
fn later_edits_to_the_method_dont_change_the_payment() {
    let world = World::new();
    let placed = world
        .place_as(&Ctx::system(), world.cart(), None, None)
        .unwrap()
        .value;
    let mut method = block_on(world.payment_methods.get(bank_transfer()))
        .unwrap()
        .unwrap();
    method.rename("Wire").unwrap();
    method.set_instructions(None).unwrap();
    block_on(world.payment_methods.update(&mut method)).unwrap();
    let stored = block_on(world.payments.list_for_order(placed.order.id())).unwrap();
    assert_eq!(stored[0].method_name(), Some("Bank transfer"));
    assert!(stored[0].instructions().is_some());
}

#[test]
fn checkout_needs_a_payment_method() {
    let world = World::new();
    let cart = cart_without_method(&world);
    assert_eq!(
        world.checkout(cart),
        Err(CommandError::PaymentMethodRequired(cart))
    );
    assert_eq!(world.mug_reserved(), 0);
}

#[test]
fn a_method_no_longer_offered_stops_checkout() {
    let world = World::new();
    let cart = world.cart();
    let mut method = block_on(world.payment_methods.get(bank_transfer()))
        .unwrap()
        .unwrap();
    method.set_enabled(false);
    block_on(world.payment_methods.update(&mut method)).unwrap();
    assert_eq!(
        world.checkout(cart),
        Err(CommandError::PaymentMethodUnavailable(bank_transfer()))
    );
    method.set_enabled(true);
    method.set_markets(vec![MarketId::from_uuid(Uuid::from_u128(4444))]);
    block_on(world.payment_methods.update(&mut method)).unwrap();
    assert_eq!(
        world.checkout(cart),
        Err(CommandError::PaymentMethodUnavailable(bank_transfer()))
    );
}

#[test]
fn a_method_of_a_provider_not_in_this_build_is_unavailable() {
    let world = World::new();
    let cart = world.cart();
    let mut method = block_on(world.payment_methods.get(bank_transfer()))
        .unwrap()
        .unwrap();
    let mut s = method.snapshot();
    s.provider = ProviderKey::new("app_stripe").unwrap();
    s.version = method.version();
    method = PaymentMethod::restore(s).unwrap();
    block_on(world.payment_methods.update(&mut method)).unwrap();
    assert_eq!(
        world.checkout(cart),
        Err(CommandError::PaymentMethodUnavailable(bank_transfer()))
    );
}

#[test]
fn nothing_to_pay_needs_no_method_and_makes_no_payment() {
    let world = World::new();
    let mut free = Product::new_simple(
        ProductId::from_uuid(Uuid::from_u128(1110)),
        Handle::new("sticker").unwrap(),
        "Sticker".to_string(),
        VariantId::from_uuid(Uuid::from_u128(1210)),
        Price::new(eur(0)).unwrap(),
    )
    .unwrap();
    not_shipped(&mut free, VariantId::from_uuid(Uuid::from_u128(1210)));
    free.set_status(ProductStatus::Active).unwrap();
    block_on(world.products.insert(&mut free)).unwrap();
    let catalog = hypha_core::CatalogId::from_uuid(Uuid::from_u128(1003));
    block_on(world.catalogs.add_products(catalog, &[free.id()])).unwrap();

    let ctx = world.ports.storefront();
    let input = CreateCart {
        country: "HR".to_string(),
        idempotency: None,
        customer: None,
    };
    let cart = block_on(create_cart(
        &ctx,
        &world.ports,
        &world.ids,
        &world.clock,
        input,
    ))
    .unwrap()
    .value
    .id;
    let line = AddLine {
        cart,
        variant: free.variants()[0].id(),
        quantity: 1,
        idempotency: None,
    };
    block_on(add_line(&ctx, &world.ports, &world.clock, line)).unwrap();
    block_on(set_cart_email(
        &ctx,
        &world.ports,
        cart,
        Some(ana().to_string()),
    ))
    .unwrap();
    let placed = world.place_as(&ctx, cart, None, None).unwrap().value;
    assert_eq!(placed.order.total(), eur(0));
    assert_eq!((placed.payment, placed.next_action), (None, None));
}

#[test]
fn a_replayed_checkout_answers_with_the_same_payment() {
    let world = World::new();
    let cart = world.cart();
    let key = || {
        Some(IdempotencyRequest {
            key: "co-pay".to_string(),
            fingerprint: [2; 32],
        })
    };
    let first = world.place_as(&Ctx::system(), cart, None, key()).unwrap();
    let again = world.place_as(&Ctx::system(), cart, None, key()).unwrap();
    assert!(again.replayed);
    assert_eq!(again.value, first.value);
}

#[test]
fn a_cart_lists_the_methods_offered_to_it_by_position() {
    let world = World::new();
    let cart = world.cart();
    let cod = add_cod(&world, 1600, vec![], 0);
    add_cod(
        &world,
        1601,
        vec![MarketId::from_uuid(Uuid::from_u128(4444))],
        0,
    );
    let mut bank = block_on(world.payment_methods.get(bank_transfer()))
        .unwrap()
        .unwrap();
    bank.set_position(5);
    block_on(world.payment_methods.update(&mut bank)).unwrap();
    let listed = block_on(cart_payment_methods(
        &world.ports.storefront(),
        &world.ports,
        &(ManualProvider,),
        cart,
    ))
    .unwrap();
    let ids: Vec<PaymentMethodId> = listed.iter().map(PaymentMethod::id).collect();
    assert_eq!(ids, [cod, bank_transfer()]);
}

/// Providers that count which keys `available` was asked about.
struct Counting<P> {
    inner: P,
    asked: std::cell::RefCell<Vec<ProviderKey>>,
}

impl<P: hypha_payments::PaymentProviders> hypha_payments::PaymentProviders for Counting<P> {
    fn keys(&self) -> Vec<ProviderKey> {
        self.inner.keys()
    }
    async fn available(&self, key: &ProviderKey) -> Result<bool, hypha_core::RepoError> {
        self.asked.borrow_mut().push(key.clone());
        self.inner.available(key).await
    }
    fn check_settings(
        &self,
        key: &ProviderKey,
        settings: &serde_json::Value,
    ) -> Result<(), hypha_payments::PaymentError> {
        self.inner.check_settings(key, settings)
    }
    async fn start(
        &self,
        key: &ProviderKey,
        payment: &hypha_payments::PaymentRequest,
    ) -> Result<hypha_payments::Started, hypha_payments::PaymentError> {
        self.inner.start(key, payment).await
    }
    async fn act(
        &self,
        key: &ProviderKey,
        request: &hypha_payments::ActionRequest,
    ) -> Result<hypha_payments::ActionAnswer, hypha_payments::PaymentError> {
        self.inner.act(key, request).await
    }
    async fn status(
        &self,
        key: &ProviderKey,
        request: &hypha_payments::StatusRequest,
    ) -> Result<hypha_payments::ProviderStatus, hypha_payments::PaymentError> {
        self.inner.status(key, request).await
    }
}

#[test]
fn listing_a_carts_methods_asks_each_provider_once() {
    let world = World::new();
    let cart = world.cart();
    let cod = add_cod(&world, 1600, vec![], 0);
    let other = add_cod(&world, 1601, vec![], 2);
    // Not offered in the cart's market: its provider needn't be asked for it.
    add_cod(
        &world,
        1602,
        vec![MarketId::from_uuid(Uuid::from_u128(4444))],
        0,
    );
    let providers = Counting {
        inner: (ManualProvider,),
        asked: Default::default(),
    };
    let listed = block_on(cart_payment_methods(
        &world.ports.storefront(),
        &world.ports,
        &providers,
        cart,
    ))
    .unwrap();
    let ids: Vec<PaymentMethodId> = listed.iter().map(PaymentMethod::id).collect();
    assert_eq!(ids, [cod, bank_transfer(), other]);
    assert_eq!(providers.asked.borrow().as_slice(), [ProviderKey::manual()]);
}

#[test]
fn choosing_a_method_checks_it_is_offered() {
    let world = World::new();
    let cart = cart_without_method(&world);
    let ctx = world.ports.storefront();
    let elsewhere = add_cod(
        &world,
        1601,
        vec![MarketId::from_uuid(Uuid::from_u128(4444))],
        0,
    );
    let choose = |method| {
        block_on(set_payment_method(
            &ctx,
            &world.ports,
            &(ManualProvider,),
            cart,
            method,
        ))
    };
    assert_eq!(
        choose(Some(elsewhere)).map(|_| ()),
        Err(CommandError::PaymentMethodUnavailable(elsewhere))
    );
    let unknown = PaymentMethodId::from_uuid(Uuid::from_u128(1999));
    assert_eq!(
        choose(Some(unknown)).map(|_| ()),
        Err(CommandError::PaymentMethodNotFound(unknown))
    );
    let cod = add_cod(&world, 1600, vec![market_id()], 0);
    let priced = choose(Some(cod)).unwrap();
    assert_eq!(priced.payment_method, Some(cod));
    assert_eq!(
        block_on(world.carts.get(cart))
            .unwrap()
            .unwrap()
            .payment_method(),
        Some(cod)
    );
    assert_eq!(choose(None).unwrap().payment_method, None);
}

#[test]
fn a_checked_out_cart_keeps_its_method() {
    let world = World::new();
    let cart = world.cart();
    world.checkout(cart).unwrap();
    let order = block_on(world.carts.get(cart)).unwrap().unwrap();
    let CartStatus::Completed { order } = order.status() else {
        panic!("completed");
    };
    assert_eq!(
        block_on(set_payment_method(
            &world.ports.storefront(),
            &world.ports,
            &(ManualProvider,),
            cart,
            None
        ))
        .map(|_| ()),
        Err(CommandError::CartCompleted { cart, order })
    );
}

/// A payment app as checkout sees it through the host: `app_card`, answering a card
/// widget with an attempt named after the payment (`pi_<n>`), or failing while `down`.
/// It says the payment is in `state` when asked, and keeps the payments it was asked to
/// start and the actions it was asked to take (answering each done).
struct CardApp {
    down: std::cell::Cell<bool>,
    /// Refuses to start (as Stripe does an intent cancelled there): `ProviderFailed`.
    refuse: std::cell::Cell<bool>,
    /// Can't say where a payment is (Stripe down).
    status_down: std::cell::Cell<bool>,
    state: std::cell::Cell<ProviderState>,
    /// The attempt its status answers are about, when not the payment's own.
    status_attempt: std::cell::RefCell<Option<String>>,
    started: std::cell::RefCell<Vec<hypha_core::PaymentId>>,
    acted: std::cell::RefCell<Vec<(hypha_core::PaymentId, PaymentAction)>>,
}

impl Default for CardApp {
    fn default() -> Self {
        Self {
            down: std::cell::Cell::new(false),
            refuse: std::cell::Cell::new(false),
            status_down: std::cell::Cell::new(false),
            state: std::cell::Cell::new(ProviderState::Open),
            status_attempt: std::cell::RefCell::default(),
            started: std::cell::RefCell::default(),
            acted: std::cell::RefCell::default(),
        }
    }
}

/// The attempt `app_card` makes for a payment.
fn attempt_of(payment: hypha_core::PaymentId) -> String {
    format!("pi_{}", payment.as_uuid().as_u128())
}

impl hypha_payments::PaymentProvider for &CardApp {
    fn key(&self) -> ProviderKey {
        ProviderKey::new("app_card").unwrap()
    }

    fn check_settings(&self, _: &serde_json::Value) -> Result<(), hypha_payments::PaymentError> {
        Ok(())
    }

    async fn start(
        &self,
        payment: &hypha_payments::PaymentRequest,
    ) -> Result<hypha_payments::Started, hypha_payments::PaymentError> {
        self.started.borrow_mut().push(payment.payment);
        if self.down.get() {
            return Err(hypha_payments::PaymentError::AppUnavailable {
                app: hypha_core::AppKey::new("app_card").unwrap(),
                point: "payments.start",
                reason: "answered HTTP 503".to_string(),
            });
        }
        if self.refuse.get() {
            return Err(hypha_payments::PaymentError::ProviderFailed {
                provider: ProviderKey::new("app_card").unwrap(),
                reason: "the intent was canceled".to_string(),
            });
        }
        let mut props = serde_json::Map::new();
        props.insert("client_secret".to_string(), "pi_secret".into());
        Ok(hypha_payments::Started {
            next_action: NextAction::ClientWidget {
                element: "card-payment".to_string(),
                props,
            },
            reference: Some(attempt_of(payment.payment)),
            attempt: Some(attempt_of(payment.payment)),
        })
    }

    async fn act(
        &self,
        request: &hypha_payments::ActionRequest,
    ) -> Result<hypha_payments::ActionAnswer, hypha_payments::PaymentError> {
        self.acted
            .borrow_mut()
            .push((request.payment, request.action));
        Ok(hypha_payments::ActionAnswer::Completed { reference: None })
    }

    async fn status(
        &self,
        request: &hypha_payments::StatusRequest,
    ) -> Result<hypha_payments::ProviderStatus, hypha_payments::PaymentError> {
        if self.status_down.get() {
            return Err(hypha_payments::PaymentError::ProviderFailed {
                provider: ProviderKey::new("app_card").unwrap(),
                reason: "Stripe is down".to_string(),
            });
        }
        let attempt = self
            .status_attempt
            .borrow()
            .clone()
            .unwrap_or_else(|| attempt_of(request.payment));
        Ok(hypha_payments::ProviderStatus {
            state: self.state.get(),
            reference: Some(attempt.clone()),
            attempt: Some(attempt),
        })
    }
}

/// `cart`'s payment by card: a method of `app_card`.
fn pay_by_card(world: &World, cart: CartId) -> PaymentMethodId {
    let mut method = PaymentMethod::new(NewPaymentMethod {
        id: PaymentMethodId::from_uuid(Uuid::from_u128(1300)),
        provider: ProviderKey::new("app_card").unwrap(),
        name: "Card".to_string(),
        instructions: None,
        markets: vec![],
        channels: vec![],
        enabled: true,
        position: 1,
        settings: serde_json::json!({}),
    })
    .unwrap();
    if block_on(world.payment_methods.get(method.id()))
        .unwrap()
        .is_none()
    {
        block_on(world.payment_methods.insert(&mut method)).unwrap();
    }
    let mut stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    stored.set_payment_method(Some(method.id())).unwrap();
    block_on(world.carts.update(&mut stored)).unwrap();
    method.id()
}

fn start_paying(
    world: &World,
    cart: CartId,
    app: &CardApp,
) -> Result<CartPaymentStarted, CommandError> {
    block_on(start_cart_payment(
        &world.ports.storefront(),
        &world.ports,
        &world.ids,
        &world.clock,
        &NoRateProviders,
        &(ManualProvider, app),
        StartCartPayment {
            cart,
            expected_total: None,
            return_url: None,
        },
    ))
}

fn check_out_by(
    world: &World,
    cart: CartId,
    app: &CardApp,
) -> Result<Idempotent<Placed>, CommandError> {
    let input = Checkout {
        cart,
        expected_total: None,
        idempotency: None,
        return_url: None,
    };
    block_on(checkout(
        &Ctx::system(),
        &world.ports,
        &world.ids,
        &world.clock,
        &NoRateProviders,
        &(ManualProvider, app),
        input,
    ))
}

/// A cart paying by card whose payment was started.
fn started_cart(world: &World, app: &CardApp) -> (CartId, hypha_payments::Payment) {
    let cart = world.cart();
    pay_by_card(world, cart);
    let started = start_paying(world, cart, app).unwrap();
    (cart, started.payment)
}

#[test]
fn an_online_method_needs_its_payment_first() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    pay_by_card(&world, cart);
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::PaymentRequired(cart))
    );
    assert_eq!(world.mug_reserved(), 0);
}

#[test]
fn a_cart_payment_is_started_before_the_order() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    pay_by_card(&world, cart);
    let started = start_paying(&world, cart, &app).unwrap();
    assert!(matches!(
        started.next_action,
        Ok(NextAction::ClientWidget { ref element, .. }) if element == "card-payment"
    ));
    let payment = started.payment;
    assert_eq!(app.started.borrow().as_slice(), [payment.id()]);
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!((stored.cart(), stored.order()), (Some(cart), None));
    assert_eq!(stored.status(), PaymentStatus::Pending);
    assert_eq!(stored.amount(), eur(3000));
    let attempt = attempt_of(payment.id());
    assert_eq!(stored.attempt(), Some(attempt.as_str()));
    // Nothing reserved, no order: the money comes first.
    assert_eq!(world.mug_reserved(), 0);
    let cart = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(cart.status(), CartStatus::Open);
}

#[test]
fn starting_again_resumes_the_same_payment() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, first) = started_cart(&world, &app);
    let again = start_paying(&world, cart, &app).unwrap();
    assert_eq!(again.payment.id(), first.id());
    assert_eq!(
        app.started.borrow().as_slice(),
        [first.id(), first.id()],
        "the same payment, so the same idempotency key"
    );
}

#[test]
fn a_failed_start_leaves_the_payment_pending_and_starting_again_tries_it_again() {
    let world = World::new();
    let app = CardApp::default();
    app.down.set(true);
    let cart = world.cart();
    pay_by_card(&world, cart);
    let failed = start_paying(&world, cart, &app).unwrap();
    assert!(matches!(
        failed.next_action,
        Err(hypha_payments::PaymentError::AppUnavailable { .. })
    ));
    assert_eq!(failed.payment.status(), PaymentStatus::Pending);
    app.down.set(false);
    let again = start_paying(&world, cart, &app).unwrap();
    assert!(again.next_action.is_ok());
    assert_eq!(again.payment.id(), failed.payment.id());
}

#[test]
fn a_changed_total_makes_a_new_payment_and_releases_the_old_one() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, first) = started_cart(&world, &app);
    world.add(cart, "cup", 1);
    let second = start_paying(&world, cart, &app).unwrap().payment;
    assert_ne!(second.id(), first.id());
    assert_eq!(second.amount(), eur(3500));
    let old = block_on(world.payments.get(first.id())).unwrap().unwrap();
    assert_eq!(old.status(), PaymentStatus::Voided);
    assert_eq!(
        app.acted.borrow().as_slice(),
        [(first.id(), PaymentAction::Void)]
    );
}

#[test]
fn a_manual_method_is_placed_not_paid_first() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    assert_eq!(
        start_paying(&world, cart, &app).map(|_| ()),
        Err(CommandError::PaymentMethodOffline(bank_transfer()))
    );
}

#[test]
fn checkout_waits_while_the_provider_says_the_shopper_hasnt_paid() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::PaymentPending {
            cart,
            payment: payment.id()
        })
    );
    assert_eq!(world.mug_reserved(), 0);
}

#[test]
fn checkout_asks_the_provider_and_places_the_order_once_the_money_is_held() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.state.set(ProviderState::Authorized);
    let placed = check_out_by(&world, cart, &app).unwrap().value;
    let attached = placed.payment.unwrap();
    assert_eq!(attached.id(), payment.id());
    assert_eq!(attached.order(), Some(placed.order.id()));
    assert_eq!(attached.status(), PaymentStatus::Authorized);
    assert_eq!(placed.order.financial_status(), FinancialStatus::Authorized);
    // Paid online already: nothing more to do.
    assert_eq!(placed.next_action, None);
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored, attached);
    assert_eq!(world.mug_reserved(), 2);
}

#[test]
fn processing_money_places_the_order_with_its_payment_pending() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, _) = started_cart(&world, &app);
    app.state.set(ProviderState::Processing);
    let placed = check_out_by(&world, cart, &app).unwrap().value;
    assert_eq!(
        placed.payment.map(|p| p.status()),
        Some(PaymentStatus::Processing)
    );
    assert_eq!(placed.order.financial_status(), FinancialStatus::Pending);
}

#[test]
fn a_declined_payment_must_be_started_again() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.state.set(ProviderState::Failed);
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::PaymentFailed {
            cart,
            payment: payment.id(),
            status: PaymentStatus::Failed
        })
    );
    // A new payment then.
    let again = start_paying(&world, cart, &app).unwrap();
    assert_ne!(again.payment.id(), payment.id());
}

#[test]
fn a_report_completes_the_checkout_and_the_shopper_then_gets_that_order() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    // The provider's webhook: the payment authorized, then the checkout completed.
    let mut held = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    held.authorize(None).unwrap();
    block_on(world.payments.update(&mut held)).unwrap();
    let reporter = Ctx::new(Actor::System, vec![Scope::PaymentsWrite]);
    let complete = || {
        block_on(complete_paid_checkout(
            &reporter,
            &world.ports,
            &world.ids,
            &world.clock,
            &NoRateProviders,
            &(ManualProvider, &app),
            payment.id(),
        ))
    };
    let placed = complete().unwrap().unwrap();
    assert_eq!(
        placed.payment.as_ref().and_then(|p| p.order()),
        Some(placed.order.id())
    );
    // Nothing left to complete.
    assert_eq!(complete().map(|p| p.is_some()), Ok(false));
    // The shopper's browser comes back: the same order.
    let theirs = check_out_by(&world, cart, &app).unwrap();
    assert_eq!(theirs.value.order.id(), placed.order.id());
    assert_eq!(world.mug_reserved(), 2);
}

#[test]
fn completing_a_paid_checkout_needs_payments_write() {
    let world = World::new();
    let app = CardApp::default();
    let (_, payment) = started_cart(&world, &app);
    let shopper = Ctx::new(Actor::System, vec![Scope::Checkout]);
    assert_eq!(
        block_on(complete_paid_checkout(
            &shopper,
            &world.ports,
            &world.ids,
            &world.clock,
            &NoRateProviders,
            &(ManualProvider, &app),
            payment.id(),
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PaymentsWrite
        )))
    );
}

#[test]
fn money_is_given_back_when_the_order_cant_be_placed() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.state.set(ProviderState::Authorized);
    // The mugs sold out between paying and the order.
    let mut item = block_on(world.inventory_items.get_by_variant(vid("mug")))
        .unwrap()
        .unwrap();
    item.set_on_hand(warehouse(), 0, None).unwrap();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    assert!(matches!(
        check_out_by(&world, cart, &app),
        Err(CommandError::Unavailable { .. } | CommandError::Inventory(_))
    ));
    assert_eq!(
        app.acted.borrow().as_slice(),
        [(payment.id(), PaymentAction::Void)]
    );
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored.status(), PaymentStatus::Voided);
    let cart = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(cart.status(), CartStatus::Open);
}

#[test]
fn a_paid_cart_cant_switch_to_a_manual_method() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    let mut held = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    held.authorize(None).unwrap();
    block_on(world.payments.update(&mut held)).unwrap();
    world.ports.pay_by_hand(cart);
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::CartPaymentCommitted {
            cart,
            payment: payment.id()
        })
    );
}

#[test]
fn money_given_back_is_not_committed_and_paying_starts_again() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.state.set(ProviderState::Paid);
    let mut item = block_on(world.inventory_items.get_by_variant(vid("mug")))
        .unwrap()
        .unwrap();
    item.set_on_hand(warehouse(), 0, None).unwrap();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    assert!(check_out_by(&world, cart, &app).is_err());
    let refunded = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(refunded.status(), PaymentStatus::Refunded);
    assert_eq!(
        app.acted.borrow().as_slice(),
        [(payment.id(), PaymentAction::Refund)]
    );
    // Restocked: the shopper pays again, a new payment (the refunded one counts for
    // nothing).
    item.set_on_hand(warehouse(), 5, None).unwrap();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    app.state.set(ProviderState::Open);
    let again = start_paying(&world, cart, &app).unwrap();
    assert_ne!(again.payment.id(), payment.id());
}

#[test]
fn placing_the_order_voids_the_carts_other_pending_payments() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, card) = started_cart(&world, &app);
    // The shopper switches to bank transfer and places the order.
    world.ports.pay_by_hand(cart);
    let placed = check_out_by(&world, cart, &app).unwrap().value;
    assert_eq!(placed.payment.map(|p| p.provider().is_manual()), Some(true));
    let old = block_on(world.payments.get(card.id())).unwrap().unwrap();
    assert_eq!(old.status(), PaymentStatus::Voided);
    assert_eq!(
        app.acted.borrow().as_slice(),
        [(card.id(), PaymentAction::Void)]
    );
}

#[test]
fn a_committed_payment_of_a_cart_checked_out_with_another_goes_back() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, card) = started_cart(&world, &app);
    app.state.set(ProviderState::Authorized);
    let placed = check_out_by(&world, cart, &app).unwrap().value;
    assert_eq!(placed.payment.map(|p| p.id()), Some(card.id()));
    // Another payment of the cart, held anyway (another tab, a race).
    let mut stray = hypha_payments::Payment::new(hypha_payments::NewPayment {
        id: hypha_core::PaymentId::from_uuid(Uuid::from_u128(1900)),
        cart: Some(cart),
        order: None,
        method: Some(PaymentMethodId::from_uuid(Uuid::from_u128(1300))),
        method_name: Some("Card".to_string()),
        provider: ProviderKey::new("app_card").unwrap(),
        amount: eur(3000),
        instructions: None,
    })
    .unwrap();
    stray.authorize(None).unwrap();
    block_on(world.payments.insert(&mut stray)).unwrap();
    let reporter = Ctx::new(Actor::System, vec![Scope::PaymentsWrite]);
    let done = block_on(complete_paid_checkout(
        &reporter,
        &world.ports,
        &world.ids,
        &world.clock,
        &NoRateProviders,
        &(ManualProvider, &app),
        stray.id(),
    ));
    assert_eq!(done.map(|p| p.is_some()), Ok(false));
    assert_eq!(
        app.acted.borrow().last(),
        Some(&(stray.id(), PaymentAction::Void))
    );
    let stored = block_on(world.payments.get(stray.id())).unwrap().unwrap();
    assert_eq!(stored.status(), PaymentStatus::Voided);
}

#[test]
fn a_payment_paid_meanwhile_is_not_replaced_when_its_restart_fails() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.refuse.set(true);
    app.state.set(ProviderState::Paid);
    assert_eq!(
        start_paying(&world, cart, &app).map(|_| ()),
        Err(CommandError::CartPaymentCommitted {
            cart,
            payment: payment.id()
        })
    );
}

#[test]
fn a_restart_that_fails_while_the_provider_is_down_keeps_the_payment() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.refuse.set(true);
    app.status_down.set(true);
    let again = start_paying(&world, cart, &app).unwrap();
    assert_eq!(again.payment.id(), payment.id());
    assert!(again.next_action.is_err());
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored.status(), PaymentStatus::Pending);
}

#[test]
fn a_restart_its_provider_cancelled_makes_a_new_payment() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.refuse.set(true);
    app.state.set(ProviderState::Canceled);
    let replaced = start_paying(&world, cart, &app).unwrap();
    assert_ne!(replaced.payment.id(), payment.id());
    let old = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(old.status(), PaymentStatus::Voided);
}

/// Marks `payment` authorized, as the provider's report would.
fn hold(world: &World, payment: hypha_core::PaymentId) {
    let mut held = block_on(world.payments.get(payment)).unwrap().unwrap();
    held.authorize(None).unwrap();
    block_on(world.payments.update(&mut held)).unwrap();
}

#[test]
fn a_held_payment_for_another_total_is_refused_and_given_back() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    hold(&world, payment.id());
    world.add(cart, "cup", 1);
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::TotalChanged {
            expected: eur(3000),
            actual: eur(3500)
        })
    );
    assert_eq!(world.mug_reserved(), 0);
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(
        (stored.order(), stored.status()),
        (None, PaymentStatus::Voided)
    );
}

#[test]
fn starting_again_while_a_payment_is_held_is_refused_without_asking_the_provider() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    hold(&world, payment.id());
    assert_eq!(
        start_paying(&world, cart, &app).map(|_| ()),
        Err(CommandError::CartPaymentCommitted {
            cart,
            payment: payment.id()
        })
    );
    assert_eq!(app.started.borrow().as_slice(), [payment.id()]);
}

#[test]
fn a_held_payment_cant_pay_for_another_online_method() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    hold(&world, payment.id());
    let mut wallet = PaymentMethod::new(NewPaymentMethod {
        id: PaymentMethodId::from_uuid(Uuid::from_u128(1301)),
        provider: ProviderKey::new("app_card").unwrap(),
        name: "Wallet".to_string(),
        instructions: None,
        markets: vec![],
        channels: vec![],
        enabled: true,
        position: 2,
        settings: serde_json::json!({}),
    })
    .unwrap();
    block_on(world.payment_methods.insert(&mut wallet)).unwrap();
    let mut stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    stored.set_payment_method(Some(wallet.id())).unwrap();
    block_on(world.carts.update(&mut stored)).unwrap();
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::CartPaymentCommitted {
            cart,
            payment: payment.id()
        })
    );
    assert_eq!(world.mug_reserved(), 0);
    let kept = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(
        (kept.order(), kept.status()),
        (None, PaymentStatus::Authorized)
    );
}

/// Checks `cart` out while the provider says `state` about an attempt other than its
/// payment's: the payment doesn't move.
fn another_attempt_says(state: ProviderState) {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = started_cart(&world, &app);
    app.state.set(state);
    *app.status_attempt.borrow_mut() = Some("pi_other".to_string());
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::PaymentPending {
            cart,
            payment: payment.id()
        })
    );
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored.status(), PaymentStatus::Pending);
    assert_eq!(world.mug_reserved(), 0);
}

#[test]
fn another_attempt_paid_doesnt_pay_the_payment() {
    another_attempt_says(ProviderState::Paid);
}

#[test]
fn another_attempt_failed_doesnt_fail_the_payment() {
    another_attempt_says(ProviderState::Failed);
}

#[test]
fn another_attempt_cancelled_doesnt_void_the_payment() {
    another_attempt_says(ProviderState::Canceled);
}

#[test]
fn storage_failures_dont_give_money_back() {
    let storage = CommandError::Cart(hypha_cart::CommandError::Repo(
        hypha_core::RepoError::Backend("down".to_string()),
    ));
    assert!(!storage.gives_money_back());
    assert!(
        !CommandError::Repo(hypha_core::RepoError::Backend("down".to_string())).gives_money_back()
    );
    assert!(
        CommandError::TotalChanged {
            expected: eur(1),
            actual: eur(2)
        }
        .gives_money_back()
    );
}

// ---- scopes of the cart's payment commands ----

fn caller_with(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(7))), scopes)
}

fn missing(scope: Scope) -> Result<(), CommandError> {
    Err(CommandError::Auth(AuthError::MissingScope(scope)))
}

#[test]
fn starting_a_cart_payment_needs_checkout() {
    let world = World::new();
    let cart = world.cart();
    let ctx = caller_with(vec![Scope::CartsWrite, Scope::PaymentsWrite]);
    let started = block_on(start_cart_payment(
        &ctx,
        &world.ports,
        &world.ids,
        &world.clock,
        &NoRateProviders,
        &(ManualProvider,),
        StartCartPayment {
            cart,
            expected_total: None,
            return_url: None,
        },
    ));
    assert_eq!(started.map(|_| ()), missing(Scope::Checkout));
}

#[test]
fn choosing_a_payment_method_needs_carts_write() {
    let world = World::new();
    let cart = world.cart();
    let ctx = caller_with(vec![Scope::CartsRead, Scope::Checkout]);
    let chosen = block_on(set_payment_method(
        &ctx,
        &world.ports,
        &(ManualProvider,),
        cart,
        None,
    ));
    assert_eq!(chosen.map(|_| ()), missing(Scope::CartsWrite));
}

#[test]
fn listing_a_carts_payment_methods_needs_carts_read() {
    let world = World::new();
    let cart = world.cart();
    let ctx = caller_with(vec![Scope::Checkout]);
    let listed = block_on(cart_payment_methods(
        &ctx,
        &world.ports,
        &(ManualProvider,),
        cart,
    ));
    assert_eq!(listed.map(|_| ()), missing(Scope::CartsRead));
}

#[test]
fn viewing_a_cart_needs_carts_read() {
    let world = World::new();
    let cart = world.cart();
    let ctx = caller_with(vec![Scope::Checkout]);
    let viewed = block_on(view_cart(&ctx, &world.ports, cart));
    assert_eq!(viewed.map(|_| ()), missing(Scope::CartsRead));
}

// ---- discounts ----

/// 10% off the order, automatically, with `limits`.
fn ten_percent_off(world: &World, limits: hypha_discounts::Limits) -> hypha_core::DiscountId {
    use hypha_discounts::{
        Discount, DiscountDraft, DiscountRepo, Kind, Method, Reward, Target, Value,
    };
    let kind = Kind::Standard {
        rewards: vec![Reward {
            target: Target::Subtotal,
            value: Value::Percent(hypha_core::BasisPoints::new(1000).unwrap()),
        }],
    };
    let mut draft = DiscountDraft::new("Welcome", Method::Automatic, kind);
    draft.limits = limits;
    let id = hypha_core::DiscountId::from_uuid(Uuid::from_u128(9000));
    let mut discount = Discount::new(id, draft).unwrap();
    block_on(world.discounts.insert(&mut discount)).unwrap();
    id
}

/// A use of `discount` by `customer` on another order.
fn used_elsewhere(world: &World, discount: hypha_core::DiscountId, customer: CustomerId) {
    use hypha_core::HasBatch;
    use hypha_discounts::{StageDiscounts, UsageClaim};
    let mut batch = world.ports.batch();
    batch.claim_discount_usage(UsageClaim {
        discount,
        code: None,
        order: hypha_core::OrderId::from_uuid(Uuid::from_u128(9100)),
        customer,
        once_per_customer: true,
        amount: eur(100),
        at: world.clock.now(),
    });
    block_on(world.ports.commit(batch)).unwrap();
}

#[test]
fn two_discounts_on_one_order_each_count_a_use() {
    use hypha_discounts::{
        ClassSet, Discount, DiscountDraft, DiscountRepo, ItemSet, Kind, Method, Reward, Target,
        Value,
    };
    let world = World::new();
    let offer = |n: u128, target: Target| {
        let kind = Kind::Standard {
            rewards: vec![Reward {
                target,
                value: Value::Percent(hypha_core::BasisPoints::new(1000).unwrap()),
            }],
        };
        let mut draft = DiscountDraft::new(&format!("Offer {n}"), Method::Automatic, kind);
        draft.combines_with = ClassSet::ALL;
        let id = hypha_core::DiscountId::from_uuid(Uuid::from_u128(n));
        block_on(
            world
                .discounts
                .insert(&mut Discount::new(id, draft).unwrap()),
        )
        .unwrap();
        id
    };
    let items = offer(9000, Target::Items(ItemSet::all()));
    let order = offer(9001, Target::Subtotal);
    world.checkout(world.cart()).unwrap();
    assert_eq!(
        block_on(world.discounts.usage_counts(&[items, order])),
        Ok([(items, 1), (order, 1)].into())
    );
}

#[test]
fn a_discount_reaches_the_order_with_its_shares_and_counts_a_use() {
    use hypha_discounts::DiscountRepo;
    let world = World::new();
    let discount = ten_percent_off(
        &world,
        hypha_discounts::Limits {
            total: Some(5),
            once_per_customer: false,
        },
    );
    let order = world.checkout(world.cart()).unwrap();
    // 24.00 less 10%: 2.00 off the mugs, 0.40 off the cup; VAT on what's left.
    assert_eq!(
        (
            order.discount_total(),
            order.subtotal(),
            order.tax_total(),
            order.total()
        ),
        (eur(240), eur(2160), eur(540), eur(2700))
    );
    assert_eq!(
        order
            .lines()
            .iter()
            .map(|l| (l.subtotal, l.total))
            .collect::<Vec<_>>(),
        vec![(eur(2000), eur(1800)), (eur(400), eur(360))]
    );
    assert_eq!(order.discounts()[0].discount, discount);
    assert_eq!(order.discounts()[0].amount, eur(240));
    assert_eq!(
        block_on(world.discounts.usage_counts(&[discount])),
        Ok([(discount, 1)].into())
    );
    let usage = block_on(world.discounts.usage_page(
        discount,
        hypha_core::PageRequest {
            after: None,
            limit: hypha_core::PageLimit::DEFAULT,
        },
    ))
    .unwrap();
    assert_eq!(
        (
            usage.items[0].order,
            usage.items[0].customer,
            usage.items[0].amount
        ),
        (order.id(), order.customer(), eur(240))
    );
}

#[test]
fn losing_the_last_use_is_discounts_changed_and_places_nothing() {
    let world = World::new();
    let discount = ten_percent_off(
        &world,
        hypha_discounts::Limits {
            total: Some(1),
            once_per_customer: false,
        },
    );
    let cart = world.cart();
    used_elsewhere(
        &world,
        discount,
        CustomerId::from_uuid(Uuid::from_u128(9200)),
    );
    // Checkout reads the cart before the other use shows.
    world.discounts.hide_next_usage_counts(1);
    assert_eq!(
        world.checkout(cart),
        Err(CommandError::DiscountsChanged {
            total: eur(3000),
            codes: vec![],
        })
    );
    let stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    assert_eq!(stored.status(), CartStatus::Open);
    assert_eq!(world.mug_reserved(), 0);
}

/// A cart paying 27.00 by card (10% off, its last use) whose payment the provider
/// authorized, then the last use taken by another order before checkout reads it.
fn paid_then_last_use_taken(world: &World, app: &CardApp) -> (CartId, hypha_payments::Payment) {
    let discount = ten_percent_off(
        world,
        hypha_discounts::Limits {
            total: Some(1),
            once_per_customer: false,
        },
    );
    let (cart, payment) = started_cart(world, app);
    assert_eq!(payment.amount(), eur(2700));
    app.state.set(ProviderState::Authorized);
    let mut held = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    held.authorize(None).unwrap();
    block_on(world.payments.update(&mut held)).unwrap();
    used_elsewhere(
        world,
        discount,
        CustomerId::from_uuid(Uuid::from_u128(9200)),
    );
    // Checkout reads the cart before the other use shows, so only the claim catches it.
    world.discounts.hide_next_usage_counts(1);
    (cart, payment)
}

#[test]
fn a_paid_cart_losing_its_discounts_last_use_gets_its_money_back() {
    let world = World::new();
    let app = CardApp::default();
    let (cart, payment) = paid_then_last_use_taken(&world, &app);
    assert_eq!(
        check_out_by(&world, cart, &app).map(|_| ()),
        Err(CommandError::DiscountsChanged {
            total: eur(3000),
            codes: vec![],
        })
    );
    assert_eq!(
        app.acted.borrow().as_slice(),
        [(payment.id(), PaymentAction::Void)]
    );
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored.status(), PaymentStatus::Voided);
    assert_eq!(world.mug_reserved(), 0);
}

#[test]
fn a_report_completing_a_cart_that_lost_its_discounts_last_use_gives_the_money_back() {
    let world = World::new();
    let app = CardApp::default();
    let (_, payment) = paid_then_last_use_taken(&world, &app);
    let reporter = Ctx::new(Actor::System, vec![Scope::PaymentsWrite]);
    assert_eq!(
        block_on(complete_paid_checkout(
            &reporter,
            &world.ports,
            &world.ids,
            &world.clock,
            &NoRateProviders,
            &(ManualProvider, &app),
            payment.id(),
        ))
        .map(|_| ()),
        Err(CommandError::DiscountsChanged {
            total: eur(3000),
            codes: vec![],
        })
    );
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored.status(), PaymentStatus::Voided);
}

#[test]
fn a_guest_who_used_a_once_per_customer_discount_doesnt_get_it_again() {
    let world = World::new();
    let discount = ten_percent_off(
        &world,
        hypha_discounts::Limits {
            total: None,
            once_per_customer: true,
        },
    );
    let mut ana_customer = Customer::new(CustomerId::from_uuid(Uuid::from_u128(9300)), ana());
    block_on(world.customers.insert(&mut ana_customer)).unwrap();
    used_elsewhere(&world, discount, ana_customer.id());
    let order = world.checkout(world.cart()).unwrap();
    assert_eq!((order.discount_total(), order.total()), (eur(0), eur(3000)));
}

#[test]
fn a_checked_out_cart_reads_with_its_orders_discounts() {
    let world = World::new();
    ten_percent_off(&world, hypha_discounts::Limits::default());
    let cart = world.cart();
    world.checkout(cart).unwrap();
    let viewed = block_on(view_cart(&Ctx::system(), &world.ports, cart)).unwrap();
    assert_eq!(
        (viewed.discount_total, viewed.discounts.len(), viewed.total),
        (eur(240), 1, eur(2700))
    );
    assert_eq!(viewed.discounts[0].title, "Welcome");
}
