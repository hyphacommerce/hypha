//! Checkout with shipping (design note 0015, section 7, SH9, SH12).

use std::sync::Mutex;

use hypha_cart::{
    AddLine, CartRepo, CreateCart, SelectedShipping, StoredQuote, add_line, create_cart, get_cart,
    select_shipping, set_cart_email, set_shipping_address,
};
use hypha_checkout::{Checkout, CommandError, checkout};
use hypha_core::{
    AddressParts, BasisPoints, CartId, CatalogId, Clock, CountryCode, Ctx, Currency, CustomerId,
    Email, MarketId, Money, Price, PriceListId, ProductId, Quantity, ShippingGroupId,
    ShippingOptionId, ShippingZoneId, VariantId,
};
use hypha_customers::{Customer, CustomerRepo};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus, VariantPatch};
use hypha_shipping::{
    BuiltinRate, NewOption, OptionPatch, OptionRate, ProviderError, ProviderKey, Rate,
    RateProvider, RateRequest, ServiceInfo, ShippingZone, ShippingZoneRepo,
};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn hr() -> CountryCode {
    CountryCode::new("HR").unwrap()
}
fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::Eur).unwrap()
}
fn price(amount: i64) -> Price {
    Price::new(eur(amount)).unwrap()
}
fn mug() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(1200))
}
fn ebook() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(1201))
}
fn zone_id() -> ShippingZoneId {
    ShippingZoneId::from_uuid(Uuid::from_u128(1001))
}
fn standard() -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(1010))
}
fn express() -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(1011))
}
fn dhl() -> ProviderKey {
    ProviderKey::new("app_dhl").unwrap()
}

/// DHL's `express`, at a price the test sets.
struct Dhl(Mutex<i64>);

impl RateProvider for Dhl {
    async fn services(&self, _: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        Ok(vec![])
    }
    async fn quote(&self, _: &ProviderKey, _: &RateRequest) -> Result<Vec<Rate>, ProviderError> {
        Ok(vec![Rate {
            service: "express".to_string(),
            name: "Express".to_string(),
            amount: price(*self.0.lock().unwrap()),
            estimate: None,
            reference: Some("q".to_string()),
        }])
    }
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
    dhl: Dhl,
}

impl World {
    /// Croatia (EUR, prices without tax): the mug (10.00, shipped) and the e-book (5.00,
    /// not). Croatia's zone: Standard, flat 4.00; Express, DHL's.
    fn new() -> Self {
        let w = Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
            dhl: Dhl(Mutex::new(1290)),
        };
        let p = &w.ports;
        let market_id = MarketId::from_uuid(Uuid::from_u128(1000));
        let mut market =
            Market::new(market_id, "Eurozone".to_string(), vec![hr()], Currency::Eur).unwrap();
        block_on(p.markets.insert(&mut market)).unwrap();
        for (n, variant, amount, shipped) in
            [(1100, mug(), 1000, true), (1101, ebook(), 500, false)]
        {
            let mut product = Product::new_simple(
                ProductId::from_uuid(Uuid::from_u128(n)),
                Handle::new(&format!("p{n}")).unwrap(),
                "Thing".to_string(),
                variant,
                price(amount),
            )
            .unwrap();
            product
                .update_variant(
                    variant,
                    VariantPatch {
                        requires_shipping: Some(shipped),
                        ..VariantPatch::default()
                    },
                )
                .unwrap();
            product.set_status(ProductStatus::Active).unwrap();
            block_on(p.products.insert(&mut product)).unwrap();
        }
        let catalog_id = CatalogId::from_uuid(Uuid::from_u128(1002));
        let list_id = PriceListId::from_uuid(Uuid::from_u128(1003));
        let mut catalog =
            Catalog::new(catalog_id, CatalogContext::Market(market_id), Some(list_id));
        block_on(p.catalogs.insert(&mut catalog)).unwrap();
        let products = [
            ProductId::from_uuid(Uuid::from_u128(1100)),
            ProductId::from_uuid(Uuid::from_u128(1101)),
        ];
        block_on(p.catalogs.add_products(catalog_id, &products)).unwrap();
        let mut list = PriceList::new(list_id, Currency::Eur, BasisPoints::new(0).unwrap());
        block_on(p.price_lists.insert(&mut list)).unwrap();

        let mut zone = ShippingZone::new(zone_id(), "Croatia", vec![hr()]).unwrap();
        for (id, code, rate) in [
            (
                standard(),
                "standard",
                OptionRate::Builtin(BuiltinRate::flat(vec![price(400)]).unwrap()),
            ),
            (
                express(),
                "express",
                OptionRate::Provider {
                    provider: dhl(),
                    service: "express".to_string(),
                },
            ),
        ] {
            zone.add_option(NewOption {
                id,
                name: code.to_string(),
                code: code.to_string(),
                rate,
                estimate: None,
                tax_class: None,
                is_backup: false,
                enabled: true,
            })
            .unwrap();
        }
        block_on(p.shipping_zones.insert(&mut zone)).unwrap();
        w
    }

    /// A cart of one `variant` with an email, ready but for shipping.
    fn cart_with(&self, variant: VariantId) -> CartId {
        let input = CreateCart {
            country: "HR".to_string(),
            idempotency: None,
        };
        let cart = block_on(create_cart(
            &Ctx::system(),
            &self.ports,
            &self.ids,
            &self.clock,
            input,
        ))
        .unwrap()
        .value
        .id;
        let line = AddLine {
            cart,
            variant,
            quantity: 1,
            idempotency: None,
        };
        block_on(add_line(&Ctx::system(), &self.ports, &self.clock, line)).unwrap();
        block_on(set_cart_email(
            &Ctx::system(),
            &self.ports,
            cart,
            Some("ana@example.hr".to_string()),
        ))
        .unwrap();
        cart
    }

    fn ship_to_zagreb(&self, cart: CartId) {
        let address = AddressParts {
            first_name: None,
            last_name: "Horvat".to_string(),
            company: None,
            line1: "Ilica 1".to_string(),
            line2: None,
            city: "Zagreb".to_string(),
            province: None,
            postal_code: None,
            country: "HR".to_string(),
            phone: None,
        };
        block_on(set_shipping_address(
            &Ctx::system(),
            &self.ports,
            cart,
            Some(address),
        ))
        .unwrap();
    }

    fn choose(&self, cart: CartId, option: ShippingOptionId) {
        let group = ShippingGroupId::from_uuid(cart.as_uuid());
        block_on(select_shipping(
            &Ctx::system(),
            &self.ports,
            &self.dhl,
            &self.clock,
            cart,
            group,
            option,
        ))
        .unwrap();
    }

    fn check_out(
        &self,
        cart: CartId,
        expected: Option<Money>,
    ) -> Result<hypha_orders::Order, CommandError> {
        let input = Checkout {
            cart,
            expected_total: expected,
            idempotency: None,
        };
        block_on(checkout(
            &Ctx::system(),
            &self.ports,
            &self.ids,
            &self.clock,
            &self.dhl,
            input,
        ))
        .map(|o| o.value)
    }
}

#[test]
fn shipped_lines_need_an_address_and_a_choice() {
    let w = World::new();
    let cart = w.cart_with(mug());
    assert_eq!(
        w.check_out(cart, None).map(|_| ()),
        Err(CommandError::ShippingAddressRequired(cart))
    );
    w.ship_to_zagreb(cart);
    assert_eq!(
        w.check_out(cart, None).map(|_| ()),
        Err(CommandError::ShippingOptionRequired(cart))
    );
}

#[test]
fn the_order_keeps_what_shipping_cost() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, standard());
    let order = w.check_out(cart, Some(eur(1400))).unwrap();
    assert_eq!(
        (order.shipping_total(), order.total()),
        (eur(400), eur(1400))
    );
    let line = &order.shipping_lines()[0];
    assert_eq!(
        (
            line.option,
            line.code.as_str(),
            line.amount,
            line.provider.as_deref()
        ),
        (standard(), "standard", price(400), None)
    );
    assert_eq!(line.lines, vec![order.lines()[0].id]);
    assert_eq!(
        order.shipping_address().map(|a| a.parts().city.as_str()),
        Some("Zagreb")
    );
}

#[test]
fn a_cart_of_things_never_shipped_checks_out_without_an_address() {
    let w = World::new();
    let cart = w.cart_with(ebook());
    let order = w.check_out(cart, None).unwrap();
    assert!(order.shipping_lines().is_empty());
    assert_eq!(order.shipping_total(), eur(0));
}

#[test]
fn a_fresh_quote_is_charged_as_quoted() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    *w.dhl.0.lock().unwrap() = 9999;
    let order = w.check_out(cart, Some(eur(2290))).unwrap();
    assert_eq!(order.shipping_total(), eur(1290));
    assert_eq!(
        order.shipping_lines()[0].provider.as_deref(),
        Some("app_dhl")
    );
}

#[test]
fn an_expired_quote_at_a_new_price_needs_the_new_total_confirmed() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    w.clock.set(FixedClock::DEFAULT.plus_secs(15 * 60));
    *w.dhl.0.lock().unwrap() = 1500;
    assert_eq!(
        w.check_out(cart, Some(eur(2290))).map(|_| ()),
        Err(CommandError::TotalChanged {
            expected: eur(2290),
            actual: eur(2500),
        })
    );
    // The new quote is on the cart now: the buyer sees it and confirms it.
    let priced = block_on(get_cart(&Ctx::system(), &w.ports, cart)).unwrap();
    assert_eq!(priced.total, eur(2500));
    let order = w.check_out(cart, Some(eur(2500))).unwrap();
    assert_eq!(order.shipping_total(), eur(1500));
}

#[test]
fn an_expired_quote_at_the_same_price_just_goes_through() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    w.clock.set(FixedClock::DEFAULT.plus_secs(60 * 60));
    let order = w.check_out(cart, Some(eur(2290))).unwrap();
    assert_eq!(order.shipping_total(), eur(1290));
    // The renewed quote went into the cart with the order.
    let stored = block_on(w.ports.carts.get(cart)).unwrap().unwrap();
    let quote = stored.shipping().unwrap().quote.clone().unwrap();
    assert_eq!(quote.quoted_at, FixedClock::DEFAULT.plus_secs(60 * 60));
}

#[test]
fn a_disabled_option_stops_checkout() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, standard());
    let mut zone = block_on(w.ports.shipping_zones.get(zone_id()))
        .unwrap()
        .unwrap();
    zone.update_option(
        standard(),
        OptionPatch {
            enabled: Some(false),
            ..OptionPatch::default()
        },
    )
    .unwrap();
    block_on(w.ports.shipping_zones.update(&mut zone)).unwrap();
    assert_eq!(
        w.check_out(cart, None).map(|_| ()),
        Err(CommandError::ShippingUnavailable(
            hypha_cart::ShippingProblem::OptionUnavailable(standard())
        ))
    );
}

#[test]
fn an_unavailable_line_is_reported_before_an_expired_quote_is_renewed() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    w.clock.set(FixedClock::DEFAULT.plus_secs(60 * 60));
    let id = ProductId::from_uuid(Uuid::from_u128(1100));
    let mut product = block_on(w.ports.products.get(id)).unwrap().unwrap();
    product.set_status(ProductStatus::Archived).unwrap();
    block_on(w.ports.products.update(&mut product)).unwrap();
    assert_eq!(
        w.check_out(cart, None).map(|_| ()),
        Err(CommandError::Unavailable { lines: vec![mug()] })
    );
}

#[test]
fn an_expired_quote_whose_zone_is_gone_means_choosing_again() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    w.clock.set(FixedClock::DEFAULT.plus_secs(60 * 60));
    let mut zone = block_on(w.ports.shipping_zones.get(zone_id()))
        .unwrap()
        .unwrap();
    block_on(w.ports.shipping_zones.delete(&mut zone)).unwrap();
    assert_eq!(
        w.check_out(cart, None).map(|_| ()),
        Err(CommandError::ShippingUnavailable(
            hypha_cart::ShippingProblem::OptionUnavailable(express())
        ))
    );
}

/// What another request does while checkout waits for DHL's first answer.
enum Race {
    /// The buyer sets the mugs to 2 and picks Express again, at DHL's 20.00 for two.
    CartChanged,
    /// Someone creates the guest's customer, so checkout's commit fails on its email.
    CustomerCreated,
}

/// DHL answering 12.90, then 20.00; the race happens during the first call.
struct Racing<'a> {
    ports: &'a InMemoryPorts,
    clock: &'a FixedClock,
    cart: CartId,
    race: Race,
    calls: Mutex<u32>,
}

impl RateProvider for Racing<'_> {
    async fn services(&self, _: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        Ok(vec![])
    }
    async fn quote(&self, _: &ProviderKey, _: &RateRequest) -> Result<Vec<Rate>, ProviderError> {
        let call = {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            *calls
        };
        if call == 1 {
            match self.race {
                Race::CartChanged => {
                    let mut cart = self.ports.carts.get(self.cart).await.unwrap().unwrap();
                    cart.set_quantity(mug(), Quantity::new(2).unwrap()).unwrap();
                    let quote = StoredQuote {
                        amount: price(2000),
                        provider: dhl(),
                        service: "express".to_string(),
                        reference: None,
                        quoted_at: self.clock.now(),
                    };
                    cart.select_shipping(Some(SelectedShipping {
                        option: express(),
                        quote: Some(quote),
                    }))
                    .unwrap();
                    self.ports.carts.update(&mut cart).await.unwrap();
                }
                Race::CustomerCreated => {
                    let email = Email::new("ana@example.hr").unwrap();
                    let mut customer =
                        Customer::new(CustomerId::from_uuid(Uuid::from_u128(9000)), email);
                    self.ports.customers.insert(&mut customer).await.unwrap();
                }
            }
        }
        Ok(vec![Rate {
            service: "express".to_string(),
            name: "Express".to_string(),
            amount: price(if call == 1 { 1290 } else { 2000 }),
            estimate: None,
            reference: None,
        }])
    }
}

impl World {
    /// Checks out a cart whose Express quote expired, while `race` happens.
    fn check_out_racing(
        &self,
        cart: CartId,
        race: Race,
    ) -> (Result<hypha_orders::Order, CommandError>, u32) {
        let rates = Racing {
            ports: &self.ports,
            clock: &self.clock,
            cart,
            race,
            calls: Mutex::new(0),
        };
        let input = Checkout {
            cart,
            expected_total: None,
            idempotency: None,
        };
        let placed = block_on(checkout(
            &Ctx::system(),
            &self.ports,
            &self.ids,
            &self.clock,
            &rates,
            input,
        ))
        .map(|o| o.value);
        (placed, *rates.calls.lock().unwrap())
    }
}

#[test]
fn a_quote_renewed_for_a_cart_that_changed_meanwhile_is_not_reused() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    w.clock.set(FixedClock::DEFAULT.plus_secs(60 * 60));
    let (placed, _) = w.check_out_racing(cart, Race::CartChanged);
    let order = placed.unwrap();
    // Two mugs at the quote for two, not the 12.90 renewed for one.
    assert_eq!(order.lines()[0].quantity.value(), 2);
    assert_eq!(order.shipping_total(), eur(2000));
}

#[test]
fn a_quote_renewed_for_an_unchanged_cart_is_reused_after_a_lost_race() {
    let w = World::new();
    let cart = w.cart_with(mug());
    w.ship_to_zagreb(cart);
    w.choose(cart, express());
    w.clock.set(FixedClock::DEFAULT.plus_secs(60 * 60));
    let (placed, calls) = w.check_out_racing(cart, Race::CustomerCreated);
    assert_eq!(placed.unwrap().shipping_total(), eur(1290));
    assert_eq!(calls, 1);
}
