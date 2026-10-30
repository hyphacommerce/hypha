//! Shipping on carts: the address, the offers, the choice, and shipping in the totals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]
use std::sync::Mutex;

use hypha_cart::{
    AddLine, CartError, CartRepo, CommandError, CreateCart, PricedCart, SetLineQuantity,
    ShippingProblem, add_line, create_cart, get_cart, select_shipping, set_line_quantity,
    set_shipping_address, shipping_offers,
};
use hypha_core::{
    AddressParts, BasisPoints, CartId, CatalogId, CountryCode, Ctx, Currency, MarketId, Money,
    Price, PriceListId, ProductId, ShippingGroupId, ShippingOptionId, ShippingZoneId, TaxRate,
    TaxRegionId, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_products::{Handle, Product, ProductRepo, ProductStatus, VariantPatch};
use hypha_shipping::{
    BuiltinRate, NewOption, NotOffered, OfferOutcome, OptionRate, ProviderError, ProviderKey, Rate,
    RateProvider, RateRequest, ServiceInfo, ShippingZone, ShippingZoneRepo,
};
use hypha_tax::{TaxRegion, TaxRegionRepo};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn market_id() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(1000))
}
fn zone_id() -> ShippingZoneId {
    ShippingZoneId::from_uuid(Uuid::from_u128(1001))
}
/// Standard (flat 4.00), Express (DHL's `express`), Backup (flat 9.00, a backup).
fn standard() -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(1010))
}
fn express() -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(1011))
}
fn backup() -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(1012))
}
fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}
/// The mug (shipped) and the e-book (not).
fn mug() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(1200))
}
fn ebook() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(1201))
}
fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}
fn price(amount: i64) -> Price {
    Price::new(eur(amount)).unwrap()
}
fn dhl() -> ProviderKey {
    ProviderKey::new("app_dhl").unwrap()
}

/// What the fake DHL answers.
struct Dhl(Mutex<Result<Vec<Rate>, ProviderError>>);

impl Dhl {
    fn quoting(amount: i64) -> Self {
        Self(Mutex::new(Ok(vec![Rate {
            service: "express".to_string(),
            name: "DHL Express".to_string(),
            amount: price(amount),
            estimate: None,
            reference: Some("dhl-quote-1".to_string()),
        }])))
    }
    fn failing() -> Self {
        Self(Mutex::new(Err(ProviderError::Timeout(dhl()))))
    }
}

impl RateProvider for Dhl {
    async fn services(&self, _: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        Ok(vec![])
    }
    async fn quote(&self, _: &ProviderKey, _: &RateRequest) -> Result<Vec<Rate>, ProviderError> {
        self.0.lock().unwrap().clone()
    }
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
}

impl World {
    /// Croatia in an EUR market selling the mug (10.00) and the e-book (5.00), and a zone
    /// for Croatia with the three options. No tax region until `tax`.
    fn new() -> Self {
        let world = Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
        };
        let p = &world.ports;
        let mut market = Market::new(
            market_id(),
            "Eurozone".to_string(),
            vec![hr()],
            Currency::EUR,
        )
        .unwrap();
        block_on(p.markets.insert(&mut market)).unwrap();
        for (n, variant, amount, shipped) in
            [(1100, mug(), 1000, true), (1101, ebook(), 500, false)]
        {
            let mut product = Product::new_simple(
                pid(n),
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
        let mut catalog = Catalog::new(
            catalog_id,
            CatalogContext::Market(market_id()),
            Some(list_id),
        );
        block_on(p.catalogs.insert(&mut catalog)).unwrap();
        block_on(p.catalogs.add_products(catalog_id, &[pid(1100), pid(1101)])).unwrap();
        let mut list = PriceList::new(
            list_id,
            hypha_markets::PriceListDraft::new("Euro", Currency::EUR)
                .adjusted(BasisPoints::new(0).unwrap()),
        )
        .unwrap();
        block_on(p.price_lists.insert(&mut list)).unwrap();

        let mut zone = ShippingZone::new(zone_id(), "Croatia", vec![hr()]).unwrap();
        let option = |id, code: &str, rate, is_backup| NewOption {
            id,
            name: code.to_string(),
            code: code.to_string(),
            rate,
            estimate: None,
            tax_class: None,
            is_backup,
            enabled: true,
        };
        let flat = |amount| OptionRate::Builtin(BuiltinRate::flat(vec![price(amount)]).unwrap());
        zone.add_option(option(standard(), "standard", flat(400), false))
            .unwrap();
        let by_dhl = OptionRate::Provider {
            provider: dhl(),
            service: "express".to_string(),
        };
        zone.add_option(option(express(), "express", by_dhl, false))
            .unwrap();
        zone.add_option(option(backup(), "backup", flat(900), true))
            .unwrap();
        block_on(p.shipping_zones.insert(&mut zone)).unwrap();
        world
    }

    /// Croatia's VAT, 25 %.
    fn tax(&self) {
        let mut region = TaxRegion::new(
            TaxRegionId::from_uuid(Uuid::from_u128(1004)),
            hr(),
            TaxRate::new(250_000).unwrap(),
        );
        block_on(self.ports.tax_regions.insert(&mut region)).unwrap();
    }

    fn cart_with(&self, variant: VariantId, quantity: u32) -> CartId {
        let input = CreateCart {
            country: "HR".to_string(),
            idempotency: None,
            customer: None,
        };
        let cart = block_on(create_cart(
            &self.ports.storefront(),
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
            quantity,
            idempotency: None,
        };
        block_on(add_line(&Ctx::system(), &self.ports, &self.clock, line)).unwrap();
        cart
    }

    fn ship_to(&self, cart: CartId, country: &str) -> Result<PricedCart, CommandError> {
        block_on(set_shipping_address(
            &Ctx::system(),
            &self.ports,
            cart,
            Some(address(country)),
        ))
    }

    fn select(
        &self,
        rates: &Dhl,
        cart: CartId,
        option: ShippingOptionId,
    ) -> Result<PricedCart, CommandError> {
        block_on(select_shipping(
            &Ctx::system(),
            &self.ports,
            rates,
            &self.clock,
            cart,
            group(cart),
            option,
        ))
    }

    fn get(&self, cart: CartId) -> PricedCart {
        block_on(get_cart(&Ctx::system(), &self.ports, cart)).unwrap()
    }
}

fn hr() -> CountryCode {
    CountryCode::new("HR").unwrap()
}

fn address(country: &str) -> AddressParts {
    AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: None,
        line1: "Ilica 1".to_string(),
        line2: None,
        city: "Zagreb".to_string(),
        province: None,
        postal_code: Some("10000".to_string()),
        country: country.to_string(),
        phone: None,
    }
}

/// A cart's one group is named after it.
fn group(cart: CartId) -> ShippingGroupId {
    ShippingGroupId::from_uuid(cart.as_uuid())
}

fn offered(outcomes: &[OfferOutcome]) -> Vec<(ShippingOptionId, Money)> {
    outcomes
        .iter()
        .filter_map(|o| match o {
            OfferOutcome::Offered(offer) => Some((offer.option, offer.amount)),
            OfferOutcome::NotOffered { .. } => None,
        })
        .collect()
}

// ---- address ----

#[test]
fn an_address_in_another_country_is_refused() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    assert_eq!(
        w.ship_to(cart, "SI").map(|_| ()),
        Err(CommandError::Cart(CartError::AddressCountryMismatch {
            address: CountryCode::new("SI").unwrap(),
            cart: hr(),
        }))
    );
}

#[test]
fn the_address_is_stored_and_priced_with_the_cart() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    let priced = w.ship_to(cart, "HR").unwrap();
    assert_eq!(
        priced.shipping_address.map(|a| a.parts().city.clone()),
        Some("Zagreb".to_string())
    );
    let stored = block_on(w.ports.carts.get(cart)).unwrap().unwrap();
    assert!(stored.shipping_address().is_some());
}

// ---- offers ----

#[test]
fn offers_price_built_in_and_provider_options() {
    let w = World::new();
    let cart = w.cart_with(mug(), 2);
    w.ship_to(cart, "HR").unwrap();
    let offers = block_on(shipping_offers(
        &Ctx::system(),
        &w.ports,
        &Dhl::quoting(1290),
        cart,
    ))
    .unwrap();
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].group, group(cart));
    assert_eq!(
        offered(&offers[0].offers),
        vec![(standard(), eur(400)), (express(), eur(1290))]
    );
}

#[test]
fn the_backup_stays_hidden_while_a_regular_option_is_offered() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    let offers = block_on(shipping_offers(
        &Ctx::system(),
        &w.ports,
        &Dhl::failing(),
        cart,
    ))
    .unwrap();
    assert_eq!(offered(&offers[0].offers), vec![(standard(), eur(400))]);
}

#[test]
fn a_hidden_backup_is_not_offered_rather_than_unknown() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    assert_eq!(
        w.select(&Dhl::quoting(1290), cart, backup()).map(|_| ()),
        Err(CommandError::ShippingOptionNotOffered {
            option: backup(),
            reason: NotOffered::BackupNotNeeded,
        })
    );
}

#[test]
fn offers_need_an_address_and_something_to_ship() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    assert_eq!(
        block_on(shipping_offers(
            &Ctx::system(),
            &w.ports,
            &Dhl::failing(),
            cart
        )),
        Err(CommandError::Cart(CartError::ShippingAddressRequired))
    );
    let digital = w.cart_with(ebook(), 1);
    w.ship_to(digital, "HR").unwrap();
    assert_eq!(
        block_on(shipping_offers(
            &Ctx::system(),
            &w.ports,
            &Dhl::failing(),
            digital
        )),
        Err(CommandError::NothingToShip)
    );
}

#[test]
fn a_country_without_a_zone_has_no_shipping() {
    let w = World::new();
    let mut zone = block_on(w.ports.shipping_zones.get(zone_id()))
        .unwrap()
        .unwrap();
    block_on(w.ports.shipping_zones.delete(&mut zone)).unwrap();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    assert_eq!(
        block_on(shipping_offers(
            &Ctx::system(),
            &w.ports,
            &Dhl::failing(),
            cart
        )),
        Err(CommandError::NoShippingToCountry(hr()))
    );
}

// ---- choosing ----

#[test]
fn a_built_in_choice_is_in_the_totals() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    let priced = w.select(&Dhl::failing(), cart, standard()).unwrap();
    let group = &priced.shipping_groups[0];
    assert_eq!(group.lines, vec![mug()]);
    let selected = group.selected.as_ref().unwrap();
    assert_eq!(
        (selected.option, selected.amount, selected.quote.as_ref()),
        (standard(), eur(400), None)
    );
    assert_eq!(
        (priced.subtotal, priced.shipping_total, priced.total),
        (eur(1000), eur(400), eur(1400))
    );
    assert!(priced.is_shipping_ready());
    assert_eq!(w.get(cart), priced);
}

#[test]
fn shipping_is_taxed_like_a_line() {
    let w = World::new();
    w.tax();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    let priced = w.select(&Dhl::failing(), cart, standard()).unwrap();
    // 25 % of 10.00 and of 4.00, added on top (the market's prices exclude tax).
    assert_eq!(
        (priced.shipping_total, priced.tax_total, priced.total),
        (eur(400), eur(350), eur(1750))
    );
}

#[test]
fn a_providers_choice_keeps_its_quote() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    let dhl_rates = Dhl::quoting(1290);
    let priced = w.select(&dhl_rates, cart, express()).unwrap();
    let quote = priced.shipping_groups[0]
        .selected
        .as_ref()
        .unwrap()
        .quote
        .clone()
        .unwrap();
    assert_eq!(
        (quote.amount, quote.reference.as_deref(), quote.quoted_at),
        (price(1290), Some("dhl-quote-1"), FixedClock::DEFAULT)
    );
    assert_eq!(priced.shipping_total, eur(1290));

    // DHL's price moves; the cart keeps showing what the buyer was quoted.
    *dhl_rates.0.lock().unwrap() = Dhl::quoting(1500).0.into_inner().unwrap();
    assert_eq!(w.get(cart).shipping_total, eur(1290));
}

#[test]
fn an_option_not_offered_now_cant_be_chosen() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    assert_eq!(
        w.select(&Dhl::failing(), cart, express()).map(|_| ()),
        Err(CommandError::ShippingOptionNotOffered {
            option: express(),
            reason: NotOffered::ProviderFailed(ProviderError::Timeout(dhl())),
        })
    );
    let unknown = ShippingOptionId::from_uuid(Uuid::from_u128(9));
    assert_eq!(
        w.select(&Dhl::failing(), cart, unknown).map(|_| ()),
        Err(CommandError::ShippingOptionUnknown(unknown))
    );
    let other_group = ShippingGroupId::from_uuid(Uuid::from_u128(9));
    assert_eq!(
        block_on(select_shipping(
            &Ctx::system(),
            &w.ports,
            &Dhl::failing(),
            &w.clock,
            cart,
            other_group,
            standard()
        ))
        .map(|_| ()),
        Err(CommandError::ShippingGroupNotFound(other_group))
    );
}

#[test]
fn a_new_address_drops_the_choice() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    w.select(&Dhl::failing(), cart, standard()).unwrap();
    let mut moved = address("HR");
    moved.line1 = "Vlaška 2".to_string();
    let priced = block_on(set_shipping_address(
        &Ctx::system(),
        &w.ports,
        cart,
        Some(moved),
    ))
    .unwrap();
    assert_eq!(priced.shipping_groups[0].selected, None);
    assert!(!priced.is_shipping_ready());
}

#[test]
fn changing_lines_drops_a_quote_but_keeps_a_built_in_choice() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    w.select(&Dhl::failing(), cart, standard()).unwrap();
    let two = SetLineQuantity {
        cart,
        variant: mug(),
        quantity: 2,
    };
    let priced = block_on(set_line_quantity(&Ctx::system(), &w.ports, two)).unwrap();
    assert_eq!(
        priced.shipping_groups[0]
            .selected
            .as_ref()
            .map(|s| s.option),
        Some(standard())
    );

    w.select(&Dhl::quoting(1290), cart, express()).unwrap();
    let three = SetLineQuantity {
        cart,
        variant: mug(),
        quantity: 3,
    };
    let priced = block_on(set_line_quantity(&Ctx::system(), &w.ports, three)).unwrap();
    assert_eq!(priced.shipping_groups[0].selected, None);
}

#[test]
fn a_disabled_option_shows_as_a_problem_until_chosen_again() {
    let w = World::new();
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    w.select(&Dhl::failing(), cart, standard()).unwrap();
    let mut zone = block_on(w.ports.shipping_zones.get(zone_id()))
        .unwrap()
        .unwrap();
    zone.update_option(
        standard(),
        hypha_shipping::OptionPatch {
            enabled: Some(false),
            ..hypha_shipping::OptionPatch::default()
        },
    )
    .unwrap();
    block_on(w.ports.shipping_zones.update(&mut zone)).unwrap();
    let priced = w.get(cart);
    assert_eq!(
        priced.shipping_groups[0].problem,
        Some(ShippingProblem::OptionUnavailable(standard()))
    );
    assert_eq!((priced.shipping_total, priced.total), (eur(0), eur(1000)));
    assert!(!priced.is_shipping_ready());
}

#[test]
fn a_cart_of_things_never_shipped_has_no_groups() {
    let w = World::new();
    let cart = w.cart_with(ebook(), 1);
    let priced = w.get(cart);
    assert!(priced.shipping_groups.is_empty());
    assert!(priced.is_shipping_ready());
}

// ---- shipping discounts ----

/// Free shipping for everyone, automatically.
fn free_shipping(w: &World) {
    use hypha_discounts::{
        Discount, DiscountDraft, DiscountRepo, Kind, Method, Reward, Target, Value,
    };
    let kind = Kind::Standard {
        rewards: vec![Reward {
            target: Target::Shipping { max_amount: None },
            value: Value::Percent(hypha_core::BasisPoints::new(10_000).unwrap()),
        }],
    };
    let mut d = Discount::new(
        hypha_core::DiscountId::from_uuid(uuid::Uuid::from_u128(7000)),
        DiscountDraft::new("Free shipping", Method::Automatic, kind),
    )
    .unwrap();
    block_on(w.ports.discounts.insert(&mut d)).unwrap();
}

#[test]
fn free_shipping_keeps_the_price_and_takes_it_off_the_total() {
    let w = World::new();
    w.tax();
    free_shipping(&w);
    let cart = w.cart_with(mug(), 1);
    w.ship_to(cart, "HR").unwrap();
    let priced = w.select(&Dhl::failing(), cart, standard()).unwrap();
    let selected = priced.shipping_groups[0].selected.as_ref().unwrap();
    assert_eq!(
        (
            selected.amount,
            selected.total,
            selected.tax.map(|t| t.amount)
        ),
        (eur(400), eur(0), Some(eur(0)))
    );
    assert_eq!(
        (priced.shipping_total, priced.discount_total, priced.total),
        (eur(0), eur(400), eur(1250))
    );
}

#[test]
fn offers_show_their_price_after_shipping_discounts() {
    let w = World::new();
    free_shipping(&w);
    let cart = w.cart_with(mug(), 2);
    w.ship_to(cart, "HR").unwrap();
    let offers = block_on(shipping_offers(
        &Ctx::system(),
        &w.ports,
        &Dhl::quoting(1290),
        cart,
    ))
    .unwrap();
    // The hidden backup is the third outcome: not offered, so nothing beside it.
    assert_eq!(
        offers[0].after_discounts,
        vec![Some(eur(0)), Some(eur(0)), None]
    );
}
