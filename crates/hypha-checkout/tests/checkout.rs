use hypha_cart::{
    AddLine, CartRepo, CartStatus, CreateCart, add_line, create_cart, set_cart_customer,
    set_cart_email,
};
use hypha_checkout::{
    Checkout, CommandError, Placed, cart_payment_methods, checkout, set_payment_method, view_cart,
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
    ManualProvider, NewPaymentMethod, NextAction, PaymentMethod, PaymentMethodRepo, PaymentRepo,
    PaymentStatus, ProviderKey,
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

// ---- payments (design note 0027) ----

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
    assert_eq!(payment.order(), placed.order.id());
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
/// widget with its reference, or failing while `down`; it keeps the payments it was
/// asked to start.
#[derive(Default)]
struct CardApp {
    down: std::cell::Cell<bool>,
    /// Answers a reference the payment can't keep, so storing it fails after the
    /// commit, as a storage failure would.
    bad_reference: std::cell::Cell<bool>,
    started: std::cell::RefCell<Vec<hypha_core::PaymentId>>,
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
        let mut props = serde_json::Map::new();
        props.insert("client_secret".to_string(), "pi_1_secret".into());
        Ok(hypha_payments::Started {
            next_action: NextAction::ClientWidget {
                element: "card-payment".to_string(),
                props,
            },
            reference: Some(if self.bad_reference.get() {
                "x".repeat(10_000)
            } else {
                format!("pi_{}", payment.order_number)
            }),
            // Its IDs name attempts: the start's is kept as the payment's.
            attempt: (!self.bad_reference.get()).then(|| format!("pi_{}", payment.order_number)),
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
    block_on(world.payment_methods.insert(&mut method)).unwrap();
    let mut stored = block_on(world.carts.get(cart)).unwrap().unwrap();
    stored.set_payment_method(Some(method.id())).unwrap();
    block_on(world.carts.update(&mut stored)).unwrap();
    method.id()
}

fn check_out_by(world: &World, cart: CartId, app: &CardApp) -> Placed {
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
    .unwrap()
    .value
}

fn pay_again(
    world: &World,
    order: hypha_core::OrderId,
    app: &CardApp,
) -> Result<Placed, CommandError> {
    block_on(hypha_checkout::start_order_payment(
        &world.ports.storefront(),
        &world.ports,
        &world.ids,
        &(ManualProvider, app),
        hypha_checkout::StartPayment {
            order,
            return_url: None,
        },
    ))
}

#[test]
fn a_reference_that_cant_be_stored_after_the_commit_doesnt_fail_checkout() {
    let world = World::new();
    let app = CardApp::default();
    app.bad_reference.set(true);
    let cart = world.cart();
    pay_by_card(&world, cart);
    let placed = check_out_by(&world, cart, &app);
    // The order is placed and the shopper gets the provider's next action.
    assert!(matches!(
        placed.next_action,
        Some(Ok(NextAction::ClientWidget { ref element, .. })) if element == "card-payment"
    ));
    let payment = placed.payment.unwrap();
    assert_eq!(
        payment.reference(),
        None,
        "the answer shows what was stored"
    );
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    assert_eq!(stored.reference(), None);
    assert_eq!(stored.status(), PaymentStatus::Pending);
    assert!(
        block_on(world.orders.get(placed.order.id()))
            .unwrap()
            .is_some()
    );

    // Started again (the same key), the reference is stored then.
    app.bad_reference.set(false);
    let again = pay_again(&world, placed.order.id(), &app).unwrap();
    let number = placed.order.number().value();
    assert_eq!(
        again.payment.unwrap().reference(),
        Some(format!("pi_{number}").as_str())
    );
}

#[test]
fn an_app_method_starts_its_payment_and_keeps_the_reference() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    pay_by_card(&world, cart);
    let placed = check_out_by(&world, cart, &app);
    assert!(matches!(
        placed.next_action,
        Some(Ok(NextAction::ClientWidget { ref element, .. })) if element == "card-payment"
    ));
    let payment = placed.payment.unwrap();
    assert_eq!(app.started.borrow().as_slice(), [payment.id()]);
    let stored = block_on(world.payments.get(payment.id())).unwrap().unwrap();
    let number = placed.order.number().value();
    assert_eq!(stored.reference(), Some(format!("pi_{number}").as_str()));
    assert_eq!(stored.attempt(), Some(format!("pi_{number}").as_str()));
    assert_eq!(stored.status(), PaymentStatus::Pending);
}

#[test]
fn a_failed_start_leaves_the_order_placed_and_asking_again_starts_the_same_payment() {
    let world = World::new();
    let app = CardApp::default();
    app.down.set(true);
    let cart = world.cart();
    pay_by_card(&world, cart);
    let placed = check_out_by(&world, cart, &app);
    assert!(matches!(
        placed.next_action,
        Some(Err(hypha_payments::PaymentError::AppUnavailable { .. }))
    ));
    let payment = placed.payment.unwrap();
    assert_eq!(payment.status(), PaymentStatus::Pending);
    assert_eq!(payment.reference(), None);

    app.down.set(false);
    let again = pay_again(&world, placed.order.id(), &app).unwrap();
    assert!(matches!(
        again.next_action,
        Some(Ok(NextAction::ClientWidget { .. }))
    ));
    assert_eq!(again.payment.map(|p| p.id()), Some(payment.id()));
    assert_eq!(
        app.started.borrow().as_slice(),
        [payment.id(), payment.id()],
        "the same payment, so the same idempotency key"
    );
}

#[test]
fn after_a_failed_payment_asking_again_makes_a_new_one() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    pay_by_card(&world, cart);
    let placed = check_out_by(&world, cart, &app);
    let first = placed.payment.unwrap().id();
    let mut declined = block_on(world.payments.get(first)).unwrap().unwrap();
    declined.fail(None).unwrap();
    block_on(world.payments.update(&mut declined)).unwrap();

    let again = pay_again(&world, placed.order.id(), &app).unwrap();
    let second = again.payment.unwrap();
    assert_ne!(second.id(), first);
    assert_eq!(second.status(), PaymentStatus::Pending);
    assert_eq!(second.amount(), placed.order.total());
    let payments = block_on(world.payments.list_for_order(placed.order.id())).unwrap();
    assert_eq!(
        payments.iter().map(|p| p.id()).collect::<Vec<_>>(),
        [second.id(), first]
    );
    assert_eq!(app.started.borrow().last(), Some(&second.id()));
}

#[test]
fn a_paid_order_has_no_payment_to_start() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    pay_by_card(&world, cart);
    let placed = check_out_by(&world, cart, &app);
    let mut paid = block_on(world.payments.get(placed.payment.unwrap().id()))
        .unwrap()
        .unwrap();
    paid.pay(world.clock.now(), None).unwrap();
    block_on(world.payments.update(&mut paid)).unwrap();
    let mut order = block_on(world.orders.get(placed.order.id()))
        .unwrap()
        .unwrap();
    order.set_financial_status(FinancialStatus::Paid);
    block_on(world.orders.update(&mut order)).unwrap();
    assert_eq!(
        pay_again(&world, order.id(), &app).map(|_| ()),
        Err(CommandError::NotPayable {
            order: order.id(),
            status: FinancialStatus::Paid
        })
    );
}

#[test]
fn another_channels_order_is_not_found_when_asking_to_pay() {
    let world = World::new();
    let app = CardApp::default();
    let cart = world.cart();
    pay_by_card(&world, cart);
    let order = check_out_by(&world, cart, &app).order.id();
    let other = Ctx::in_channel(
        Actor::System,
        vec![Scope::Checkout],
        hypha_core::ChannelId::from_uuid(Uuid::from_u128(4242)),
    );
    assert_eq!(
        block_on(hypha_checkout::start_order_payment(
            &other,
            &world.ports,
            &world.ids,
            &(ManualProvider, &app),
            hypha_checkout::StartPayment {
                order,
                return_url: None
            },
        ))
        .map(|_| ()),
        Err(CommandError::OrderNotFound(order))
    );
}
