use hypha_core::{
    CartId, CountryCode, Currency, CustomerId, Email, MarketId, Money, OrderId, OrderLineId,
    PageLimit, PageRequest, Price, ProductId, Quantity, RepoError, TaxClassId, TaxRate,
    TaxRegionId, Timestamp, VariantId, Version, Versioned,
};
use hypha_orders::{
    FinancialStatus, NewOrder, Order, OrderLine, OrderNumber, OrderRepo, OrderTax, OrderTaxSource,
    repo::keys,
};
use uuid::Uuid;

use super::scenario;

/// Runs every `OrderRepo` scenario, each on a fresh repo from `make`.
pub async fn order_repo<R: OrderRepo>(make: impl AsyncFn() -> R) {
    scenario("order: insert stores every field at version 1");
    round_trips(&make().await).await;
    scenario("order: lookups by ID and number; unknown ones are None");
    lookups(&make().await).await;
    scenario("order: update stores every field and bumps the version; stale and missing fail");
    updates(&make().await).await;
    scenario("order: unique ID, number, cart and line IDs");
    unique_keys(&make().await).await;
    scenario("order: numbers start at 1001 and increase");
    numbers(&make().await).await;
    scenario("order: list pages newest first, after a cursor");
    list_pages(&make().await).await;
    scenario("order: shipping address and lines round trip, every field");
    shipping_round_trips(&make().await).await;
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::Eur).unwrap()
}

fn oid(n: u128) -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(n))
}

fn number(n: i64) -> OrderNumber {
    OrderNumber::new(n).unwrap()
}

/// Order `n` (number `n`, cart `n`) with one plain line `n * 10`: 1 × 5.00, untaxed.
pub(crate) fn order(n: u128) -> Order {
    order_with(n, vec![plain_line(n * 10)], false)
}

fn plain_line(id: u128) -> OrderLine {
    OrderLine {
        id: OrderLineId::from_uuid(Uuid::from_u128(id)),
        product: ProductId::from_uuid(Uuid::from_u128(id + 1)),
        variant: VariantId::from_uuid(Uuid::from_u128(id + 2)),
        quantity: Quantity::new(1).unwrap(),
        product_title: "Mug".to_string(),
        variant_title: None,
        sku: None,
        unit_price: Price::new(eur(500)).unwrap(),
        total: eur(500),
        tax: None,
    }
}

/// Every optional field set, the largest quantity, a class rate: what a mapping bug
/// would lose.
fn full_line(id: u128) -> OrderLine {
    OrderLine {
        id: OrderLineId::from_uuid(Uuid::from_u128(id)),
        product: ProductId::from_uuid(Uuid::from_u128(id + 1)),
        variant: VariantId::from_uuid(Uuid::from_u128(id + 2)),
        quantity: Quantity::new(u32::MAX).unwrap(),
        product_title: "Shirt".to_string(),
        variant_title: Some("M / Blue".to_string()),
        sku: Some("SH-M-BL".to_string()),
        unit_price: Price::new(eur(2)).unwrap(),
        total: eur(2 * i64::from(u32::MAX)),
        tax: Some(OrderTax {
            rate: TaxRate::new(50_000).unwrap(),
            class: Some(TaxClassId::from_uuid(Uuid::from_u128(id + 3))),
            amount: eur(7),
            source: OrderTaxSource::RateTable {
                region: TaxRegionId::from_uuid(Uuid::from_u128(id + 4)),
            },
        }),
    }
}

fn order_with(n: u128, lines: Vec<OrderLine>, taxes_included: bool) -> Order {
    let subtotal = lines
        .iter()
        .try_fold(eur(0), |sum, l| sum.checked_add(l.total))
        .unwrap();
    let tax_total = lines
        .iter()
        .filter_map(|l| l.tax.map(|t| t.amount))
        .try_fold(eur(0), |sum, t| sum.checked_add(t))
        .unwrap();
    let total = if taxes_included {
        subtotal
    } else {
        subtotal.checked_add(tax_total).unwrap()
    };
    Order::place(NewOrder {
        id: oid(n),
        number: number(n as i64),
        // A microsecond past a whole second, so precision is checked too.
        placed_at: Timestamp::from_unix_micros(1_767_225_600_000_001),
        cart: CartId::from_uuid(Uuid::from_u128(1000 + n)),
        market: MarketId::from_uuid(Uuid::from_u128(2000 + n)),
        currency: Currency::Eur,
        country: CountryCode::new("HR").unwrap(),
        taxes_included,
        customer: CustomerId::from_uuid(Uuid::from_u128(3000 + n)),
        email: Email::new("ana@example.hr").unwrap(),
        lines,
        shipping_address: None,
        shipping_lines: vec![],
        subtotal,
        shipping_total: Money::zero(Currency::Eur),
        tax_total,
        total,
    })
    .unwrap()
}

async fn round_trips<R: OrderRepo>(repo: &R) {
    let mut simple = order(1);
    repo.insert(&mut simple).await.unwrap();
    assert_eq!(simple.version(), Version::new(1));
    assert_eq!(repo.get(simple.id()).await, Ok(Some(simple)));

    // Line order, every optional field and taxes included must survive.
    let mut full = order_with(2, vec![full_line(21), plain_line(22)], true);
    repo.insert(&mut full).await.unwrap();
    assert_eq!(repo.get(full.id()).await, Ok(Some(full)));
}

async fn lookups<R: OrderRepo>(repo: &R) {
    let mut o = order(1);
    repo.insert(&mut o).await.unwrap();
    assert_eq!(repo.get_by_number(number(1)).await, Ok(Some(o)));
    assert_eq!(repo.get(oid(99)).await, Ok(None));
    assert_eq!(repo.get_by_number(number(99)).await, Ok(None));
}

async fn updates<R: OrderRepo>(repo: &R) {
    let mut o = order(1);
    repo.insert(&mut o).await.unwrap();
    let stale = o.clone();

    o.mark_paid();
    repo.update(&mut o).await.unwrap();
    assert_eq!(o.version(), Version::new(2));
    let stored = repo.get(o.id()).await.unwrap().unwrap();
    assert_eq!(stored.financial_status(), FinancialStatus::Paid);
    assert_eq!(stored, o);

    // Every field is written, not only what today's commands change: a changed email,
    // tax mode and lines (no command edits them yet) must round-trip too.
    let mut snapshot = order_with(1, vec![full_line(31), plain_line(32)], true).snapshot();
    snapshot.version = o.version();
    snapshot.email = Email::new("marko@example.hr").unwrap();
    snapshot.financial_status = FinancialStatus::Paid;
    let mut edited = Order::restore(snapshot).unwrap();
    repo.update(&mut edited).await.unwrap();
    assert_eq!(edited.version(), Version::new(3));
    assert_eq!(repo.get(o.id()).await, Ok(Some(edited)));

    let mut stale = stale;
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    let mut missing = order(2);
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn unique_keys<R: OrderRepo>(repo: &R) {
    let mut first = order(1);
    repo.insert(&mut first).await.unwrap();

    assert_eq!(
        repo.insert(&mut order(1)).await,
        Err(RepoError::Duplicate(keys::ORDER_ID))
    );

    let snapshot = |n: u128| order(n).snapshot();
    let mut same_number = snapshot(2);
    same_number.number = number(1);
    assert_eq!(
        repo.insert(&mut Order::restore(same_number).unwrap()).await,
        Err(RepoError::Duplicate(keys::ORDER_NUMBER))
    );

    let mut same_cart = snapshot(3);
    same_cart.cart = first.cart();
    assert_eq!(
        repo.insert(&mut Order::restore(same_cart).unwrap()).await,
        Err(RepoError::Duplicate(keys::ORDER_CART))
    );

    let mut same_line = snapshot(4);
    same_line.lines[0].id = first.lines()[0].id;
    assert_eq!(
        repo.insert(&mut Order::restore(same_line).unwrap()).await,
        Err(RepoError::Duplicate(keys::ORDER_LINE_ID))
    );
}

async fn numbers<R: OrderRepo>(repo: &R) {
    let first = repo.next_number().await.unwrap();
    assert_eq!(first, number(1001));
    let second = repo.next_number().await.unwrap();
    assert!(second > first);
}

async fn listed<R: OrderRepo>(repo: &R, after: Option<u128>, limit: u32) -> (Vec<OrderId>, bool) {
    let page = repo
        .list(PageRequest {
            after: after.map(oid),
            limit: PageLimit::new(limit).unwrap(),
        })
        .await
        .unwrap();
    (page.items.iter().map(|o| o.id).collect(), page.has_more)
}

async fn list_pages<R: OrderRepo>(repo: &R) {
    assert_eq!(listed(repo, None, 10).await, (vec![], false));

    // Orders 1 to 3, inserted out of order; 3 has two lines and is paid.
    let mut full = order_with(3, vec![full_line(31), plain_line(32)], true);
    full.mark_paid();
    repo.insert(&mut full).await.unwrap();
    for n in [1, 2] {
        repo.insert(&mut order(n)).await.unwrap();
    }

    // Summaries of the stored orders: item counts add up line quantities (order 3's
    // first line has the largest quantity, so a 32-bit sum would overflow).
    let first = repo
        .list(PageRequest {
            after: None,
            limit: PageLimit::new(2).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(
        first.items,
        vec![
            repo.get(oid(3)).await.unwrap().unwrap().summary(),
            repo.get(oid(2)).await.unwrap().unwrap().summary(),
        ]
    );
    assert_eq!(first.items[0].item_count, u64::from(u32::MAX) + 1);
    assert!(first.has_more);

    assert_eq!(listed(repo, Some(2), 2).await, (vec![oid(1)], false));
    assert_eq!(
        listed(repo, Some(3), 2).await,
        (vec![oid(2), oid(1)], false)
    );
    assert_eq!(listed(repo, Some(1), 2).await, (vec![], false));
    assert_eq!(listed(repo, Some(99), 1).await, (vec![oid(3)], true));
}

async fn shipping_round_trips<R: OrderRepo>(repo: &R) {
    use hypha_core::{
        Address, AddressParts, Price, ShippingGroupId, ShippingOptionId, TaxRate, TaxRegionId,
    };
    use hypha_orders::{OrderShippingLine, OrderTax, OrderTaxSource};

    let base = order_with(1, vec![full_line(11), plain_line(12)], false);
    let full = OrderShippingLine {
        group: ShippingGroupId::from_uuid(Uuid::from_u128(50)),
        lines: vec![
            OrderLineId::from_uuid(Uuid::from_u128(12)),
            OrderLineId::from_uuid(Uuid::from_u128(11)),
        ],
        option: ShippingOptionId::from_uuid(Uuid::from_u128(51)),
        code: "express".to_string(),
        name: "DHL Express".to_string(),
        provider: Some("spore_dhl".to_string()),
        service: Some("EXPRESS".to_string()),
        reference: Some("q-1".to_string()),
        amount: Price::new(eur(1290)).unwrap(),
        estimate_days: Some((0, u16::MAX)),
        tax: Some(OrderTax {
            rate: TaxRate::new(250_000).unwrap(),
            class: Some(hypha_core::TaxClassId::from_uuid(Uuid::from_u128(52))),
            amount: eur(323),
            source: OrderTaxSource::RateTable {
                region: TaxRegionId::from_uuid(Uuid::from_u128(53)),
            },
        }),
    };
    let plain = OrderShippingLine {
        group: ShippingGroupId::from_uuid(Uuid::from_u128(60)),
        lines: vec![],
        option: ShippingOptionId::from_uuid(Uuid::from_u128(61)),
        code: "standard".to_string(),
        name: "Standard".to_string(),
        provider: None,
        service: None,
        reference: None,
        amount: Price::new(eur(0)).unwrap(),
        estimate_days: None,
        tax: None,
    };
    let address = Address::new(AddressParts {
        first_name: Some("Ana".to_string()),
        last_name: "Horvat".to_string(),
        company: Some("Hypha".to_string()),
        line1: "Ilica 1".to_string(),
        line2: Some("Stan 4".to_string()),
        city: "Zagreb".to_string(),
        province: Some("Grad Zagreb".to_string()),
        postal_code: Some("10000".to_string()),
        country: "HR".to_string(),
        phone: Some("+385".to_string()),
    })
    .unwrap();
    let mut s = base.snapshot();
    s.version = Version::NEW;
    s.shipping_address = Some(address);
    s.shipping_lines = vec![full, plain];
    s.shipping_total = eur(1290);
    s.tax_total = s.tax_total.checked_add(eur(323)).unwrap();
    s.total = s.total.checked_add(eur(1290 + 323)).unwrap();
    let mut order = Order::restore(s).unwrap();
    repo.insert(&mut order).await.unwrap();
    assert_eq!(repo.get(order.id()).await, Ok(Some(order.clone())));

    // Updates keep it (the whole order is written).
    order.mark_paid();
    repo.update(&mut order).await.unwrap();
    assert_eq!(repo.get(order.id()).await, Ok(Some(order)));
}
