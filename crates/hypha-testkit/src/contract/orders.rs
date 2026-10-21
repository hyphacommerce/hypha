use hypha_core::{
    CartId, ChannelId, CountryCode, Currency, CustomerId, Email, Keyset, MarketId, Money, OrderId,
    OrderLineId, PageLimit, PageRequest, Price, ProductId, Quantity, RepoError, SortKey,
    TaxClassId, TaxRate, TaxRegionId, Timestamp, VariantId, Version, Versioned,
};
use hypha_orders::{
    CancelReason, FinancialStatus, FulfillmentStatus, NewOrder, Order, OrderFilter,
    OrderFilterInput, OrderLine, OrderNumber, OrderRepo, OrderSort, OrderStats, OrderTax,
    OrderTaxSource, repo::keys,
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
    scenario("order: every sort orders by its key, then ID, and pages across ties");
    list_sorted(&make().await).await;
    scenario("order: list by number or email, statuses, channel, country, currency, dates");
    list_filtered(&make().await).await;
    scenario("order: a customer's statistics add up placed orders that weren't cancelled");
    stats(&make().await).await;
    scenario("order: shipping address and lines round trip, every field");
    shipping_round_trips(&make().await).await;
    scenario("order: fulfilled quantities, the status and a cancellation round trip");
    fulfilment_round_trips(&make().await).await;
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
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
        fulfilled_quantity: 0,
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
        fulfilled_quantity: 0,
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
        channel: hypha_core::ChannelId::from_uuid(Uuid::from_u128(500 + n)),
        market: MarketId::from_uuid(Uuid::from_u128(2000 + n)),
        currency: Currency::EUR,
        country: CountryCode::new("HR").unwrap(),
        taxes_included,
        customer: CustomerId::from_uuid(Uuid::from_u128(3000 + n)),
        email: Email::new("ana@example.hr").unwrap(),
        lines,
        shipping_address: None,
        shipping_lines: vec![],
        subtotal,
        shipping_total: Money::zero(Currency::EUR),
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

    o.set_financial_status(FinancialStatus::Paid);
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

    // Every payment status is stored (design note 0027, PA7).
    for status in FinancialStatus::ALL {
        let mut current = repo.get(o.id()).await.unwrap().unwrap();
        current.set_financial_status(status);
        repo.update(&mut current).await.unwrap();
        let stored = repo.get(o.id()).await.unwrap().unwrap();
        assert_eq!(stored.financial_status(), status);
    }

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
        .list(
            &OrderFilter::default(),
            OrderSort::Newest,
            PageRequest {
                after: after.map(|n| Keyset {
                    key: SortKey::Id,
                    id: oid(n),
                }),
                limit: PageLimit::new(limit).unwrap(),
            },
        )
        .await
        .unwrap();
    (page.items.iter().map(|o| o.id).collect(), page.has_more)
}

async fn list_pages<R: OrderRepo>(repo: &R) {
    assert_eq!(listed(repo, None, 10).await, (vec![], false));

    // Orders 1 to 3, inserted out of order; 3 has two lines, is paid and partly
    // fulfilled; 2 is cancelled.
    let mut full = order_with(3, vec![full_line(31), plain_line(32)], true);
    full.set_financial_status(FinancialStatus::Paid);
    full.fulfil(&[(full.lines()[1].id, Quantity::new(1).unwrap())])
        .unwrap();
    repo.insert(&mut full).await.unwrap();
    repo.insert(&mut order(1)).await.unwrap();
    let mut cancelled = order(2);
    cancelled
        .cancel(
            Timestamp::from_unix_micros(1_767_312_000_000_001),
            CancelReason::Fraud,
            false,
        )
        .unwrap();
    repo.insert(&mut cancelled).await.unwrap();

    // Summaries of the stored orders: item counts add up line quantities (order 3's
    // first line has the largest quantity, so a 32-bit sum would overflow).
    let first = repo
        .list(
            &OrderFilter::default(),
            OrderSort::Newest,
            PageRequest {
                after: None,
                limit: PageLimit::new(2).unwrap(),
            },
        )
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

/// An order with one line of `amount` in `currency`, placed `day` days into 2026, to
/// `email` in `country` through channel `500 + channel`.
fn varied(n: u128, amount: i64, currency: Currency, email: &str, country: &str, day: i64) -> Order {
    let money = Money::new(amount, currency).unwrap();
    let line = OrderLine {
        unit_price: Price::new(money).unwrap(),
        total: money,
        ..plain_line(n * 10)
    };
    Order::place(NewOrder {
        id: oid(n),
        number: number(1000 + n as i64),
        placed_at: Timestamp::from_unix_micros(1_767_225_600_000_000 + day * 86_400_000_000),
        cart: CartId::from_uuid(Uuid::from_u128(1000 + n)),
        channel: ChannelId::from_uuid(Uuid::from_u128(500 + n % 2)),
        market: MarketId::from_uuid(Uuid::from_u128(2000 + n)),
        currency,
        country: CountryCode::new(country).unwrap(),
        taxes_included: true,
        customer: CustomerId::from_uuid(Uuid::from_u128(3000 + n)),
        email: Email::new(email).unwrap(),
        lines: vec![line],
        shipping_address: None,
        shipping_lines: vec![],
        subtotal: money,
        shipping_total: Money::zero(currency),
        tax_total: Money::zero(currency),
        total: money,
    })
    .unwrap()
}

/// `varied`, placed by customer `customer` (3000 + n otherwise).
fn by_customer(n: u128, amount: i64, currency: Currency, day: i64, customer: u128) -> Order {
    let order = varied(n, amount, currency, "ana@example.hr", "HR", day);
    let mut s = order.snapshot();
    s.customer = CustomerId::from_uuid(Uuid::from_u128(customer));
    Order::restore(s).unwrap()
}

async fn stats<R: OrderRepo>(repo: &R) {
    // Ana (customer 1): EUR on days 3 and 1, USD on day 2, and a cancelled one on day 5
    // that counts for nothing. Ivo (customer 2): one order. Nobody else.
    let mut orders = vec![
        by_customer(1, 500, Currency::EUR, 3, 1),
        by_customer(2, 250, Currency::EUR, 1, 1),
        by_customer(3, 700, Currency::USD, 2, 1),
        by_customer(4, 900, Currency::EUR, 5, 1),
        by_customer(5, 100, Currency::EUR, 4, 2),
    ];
    orders[3]
        .cancel(
            Timestamp::from_unix_micros(1_767_800_000_000_000),
            CancelReason::Customer,
            false,
        )
        .unwrap();
    for o in &mut orders {
        repo.insert(o).await.unwrap();
    }
    let ana = CustomerId::from_uuid(Uuid::from_u128(1));
    let day = |d: i64| Timestamp::from_unix_micros(1_767_225_600_000_000 + d * 86_400_000_000);
    let expected = OrderStats {
        orders_count: 3,
        first_order_at: Some(day(1)),
        last_order_at: Some(day(3)),
        amount_spent: vec![
            Money::new(750, Currency::EUR).unwrap(),
            Money::new(700, Currency::USD).unwrap(),
        ],
    };
    assert_eq!(repo.stats_for(ana).await.unwrap(), expected);
    assert_eq!(
        repo.stats_for(ana).await.unwrap(),
        OrderStats::of(orders.iter().filter(|o| o.customer() == ana)),
        "storage agrees with the reference"
    );
    assert_eq!(
        repo.stats_for(CustomerId::from_uuid(Uuid::from_u128(2)))
            .await
            .unwrap(),
        OrderStats {
            orders_count: 1,
            first_order_at: Some(day(4)),
            last_order_at: Some(day(4)),
            amount_spent: vec![Money::new(100, Currency::EUR).unwrap()],
        }
    );
    assert_eq!(
        repo.stats_for(CustomerId::from_uuid(Uuid::from_u128(9)))
            .await
            .unwrap(),
        OrderStats::default(),
        "no orders: zero, nothing, nothing"
    );
}

/// Every page of `sort`'s list under `filter`, `limit` at a time.
pub(crate) async fn all_orders<R: OrderRepo>(
    repo: &R,
    filter: &OrderFilter,
    sort: OrderSort,
    limit: u32,
) -> Vec<OrderId> {
    let mut found = Vec::new();
    let mut after = None;
    loop {
        let page = repo
            .list(
                filter,
                sort,
                PageRequest {
                    after: after.clone(),
                    limit: PageLimit::new(limit).unwrap(),
                },
            )
            .await
            .unwrap();
        found.extend(page.items.iter().map(|o| o.id));
        match page.items.last() {
            Some(last) if page.has_more => {
                after = Some(Keyset {
                    key: sort.key(last),
                    id: last.id,
                })
            }
            _ => return found,
        }
    }
}

fn filter(input: OrderFilterInput) -> OrderFilter {
    OrderFilter::new(input).unwrap()
}

async fn list_sorted<R: OrderRepo>(repo: &R) {
    // Totals in EUR with a tie (2 and 4), and one in USD that sorts apart.
    for (n, amount, currency) in [
        (3, 700, Currency::EUR),
        (1, 500, Currency::EUR),
        (4, 900, Currency::EUR),
        (2, 900, Currency::EUR),
        (5, 100, Currency::USD),
    ] {
        let mut o = varied(n, amount, currency, "a@example.com", "HR", n as i64);
        repo.insert(&mut o).await.unwrap();
    }
    let every = OrderFilter::default();
    let eur = filter(OrderFilterInput {
        currency: Some(Currency::EUR),
        ..OrderFilterInput::default()
    });
    for limit in [1, 2, 10] {
        assert_eq!(
            all_orders(repo, &every, OrderSort::Oldest, limit).await,
            [oid(1), oid(2), oid(3), oid(4), oid(5)]
        );
        assert_eq!(
            all_orders(repo, &eur, OrderSort::TotalDesc, limit).await,
            [oid(4), oid(2), oid(3), oid(1)],
            "ties by ID, descending too"
        );
        assert_eq!(
            all_orders(repo, &eur, OrderSort::TotalAsc, limit).await,
            [oid(1), oid(3), oid(2), oid(4)]
        );
    }
    assert!(matches!(
        repo.list(
            &eur,
            OrderSort::TotalAsc,
            PageRequest {
                after: Some(Keyset {
                    key: SortKey::Id,
                    id: oid(1)
                }),
                limit: PageLimit::new(2).unwrap(),
            },
        )
        .await,
        Err(RepoError::Backend(_))
    ));
}

async fn list_filtered<R: OrderRepo>(repo: &R) {
    // 1: ana, HR, day 1, paid; 2: Ivo, RS, day 2, cancelled; 3: ana in USD, day 3,
    // fulfilled; 4: ivo, HR, day 4.
    let mut ana = varied(1, 500, Currency::EUR, "Ana@Example.hr", "HR", 1);
    ana.set_financial_status(FinancialStatus::Paid);
    let mut ivo = varied(2, 500, Currency::EUR, "ivo@example.rs", "RS", 2);
    ivo.cancel(
        Timestamp::from_unix_micros(1_767_400_000_000_000),
        CancelReason::Fraud,
        false,
    )
    .unwrap();
    let mut usd = varied(3, 500, Currency::USD, "ana@example.hr", "HR", 3);
    usd.fulfil(&[(usd.lines()[0].id, Quantity::new(1).unwrap())])
        .unwrap();
    let mut later = varied(4, 500, Currency::EUR, "ivo@example.hr", "HR", 4);
    for o in [&mut ana, &mut ivo, &mut usd, &mut later] {
        repo.insert(o).await.unwrap();
    }
    let found = async |input: OrderFilterInput| {
        all_orders(repo, &filter(input), OrderSort::Oldest, 1).await
    };
    let none = OrderFilterInput::default;
    let text = |t: &str| Some(t.to_string());
    assert_eq!(
        found(OrderFilterInput {
            q: text("ANA@"),
            ..none()
        })
        .await,
        [oid(1), oid(3)],
        "email, ignoring case"
    );
    assert_eq!(
        found(OrderFilterInput {
            q: text("#1002"),
            ..none()
        })
        .await,
        [oid(2)],
        "a number"
    );
    assert_eq!(
        found(OrderFilterInput {
            q: text("100"),
            ..none()
        })
        .await,
        Vec::<OrderId>::new(),
        "a number matches whole"
    );
    assert_eq!(
        found(OrderFilterInput {
            financial_statuses: vec![FinancialStatus::Paid],
            ..none()
        })
        .await,
        [oid(1)]
    );
    assert_eq!(
        found(OrderFilterInput {
            fulfillment_statuses: vec![
                FulfillmentStatus::Fulfilled,
                FulfillmentStatus::PartiallyFulfilled
            ],
            ..none()
        })
        .await,
        [oid(3)]
    );
    assert_eq!(
        found(OrderFilterInput {
            cancelled: Some(true),
            ..none()
        })
        .await,
        [oid(2)]
    );
    assert_eq!(
        found(OrderFilterInput {
            cancelled: Some(false),
            ..none()
        })
        .await,
        [oid(1), oid(3), oid(4)]
    );
    assert_eq!(
        found(OrderFilterInput {
            customer: Some(CustomerId::from_uuid(Uuid::from_u128(3003))),
            ..none()
        })
        .await,
        [oid(3)],
        "a customer's orders"
    );
    assert_eq!(
        found(OrderFilterInput {
            channel: Some(ChannelId::from_uuid(Uuid::from_u128(500))),
            ..none()
        })
        .await,
        [oid(2), oid(4)]
    );
    assert_eq!(
        found(OrderFilterInput {
            country: Some(CountryCode::new("HR").unwrap()),
            currency: Some(Currency::EUR),
            ..none()
        })
        .await,
        [oid(1), oid(4)]
    );
    let day = |d: i64| {
        Some(Timestamp::from_unix_micros(
            1_767_225_600_000_000 + d * 86_400_000_000,
        ))
    };
    assert_eq!(
        found(OrderFilterInput {
            placed_from: day(2),
            placed_before: day(4),
            ..none()
        })
        .await,
        [oid(2), oid(3)],
        "from inclusive, before exclusive"
    );
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
    order.set_financial_status(FinancialStatus::Paid);
    repo.update(&mut order).await.unwrap();
    assert_eq!(repo.get(order.id()).await, Ok(Some(order)));
}

async fn fulfilment_round_trips<R: OrderRepo>(repo: &R) {
    // The largest quantity, half of it fulfilled: counts must survive whole.
    let mut order = order_with(1, vec![full_line(11), plain_line(12)], false);
    let big = order.lines()[0].id;
    order
        .fulfil(&[(big, Quantity::new(u32::MAX / 2).unwrap())])
        .unwrap();
    repo.insert(&mut order).await.unwrap();
    let stored = repo.get(order.id()).await.unwrap().unwrap();
    assert_eq!(
        stored.fulfillment_status(),
        FulfillmentStatus::PartiallyFulfilled
    );
    assert_eq!(stored, order);

    // Fulfilling the rest is written by an update.
    let lines: Vec<_> = order
        .lines()
        .iter()
        .map(|l| (l.id, Quantity::new(l.unfulfilled_quantity()).unwrap()))
        .collect();
    order.fulfil(&lines).unwrap();
    repo.update(&mut order).await.unwrap();
    let stored = repo.get(order.id()).await.unwrap().unwrap();
    assert_eq!(stored.fulfillment_status(), FulfillmentStatus::Fulfilled);
    assert_eq!(stored, order);

    // A cancellation, to the microsecond, with its reason.
    let mut other = order_with(2, vec![plain_line(21)], false);
    repo.insert(&mut other).await.unwrap();
    other
        .cancel(
            Timestamp::from_unix_micros(1_767_312_000_000_001),
            CancelReason::Declined,
            false,
        )
        .unwrap();
    repo.update(&mut other).await.unwrap();
    assert_eq!(repo.get(other.id()).await, Ok(Some(other)));
}
