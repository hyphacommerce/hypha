use hypha_core::{Currency, Recorded};
use uuid::Uuid;

use super::*;

fn did() -> DiscountId {
    DiscountId::from_uuid(Uuid::from_u128(1))
}

fn mkt(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(n))
}

fn eur(n: i64) -> Money {
    Money::new(n, Currency::EUR).unwrap()
}

fn bp(n: i32) -> BasisPoints {
    BasisPoints::new(n).unwrap()
}

fn standard(rewards: Vec<Reward>) -> Kind {
    Kind::Standard { rewards }
}

fn ten_percent_off_items() -> Reward {
    Reward {
        target: Target::Items(ItemSet::all()),
        value: Value::Percent(bp(1000)),
    }
}

fn draft(kind: Kind) -> DiscountDraft {
    DiscountDraft::new("Welcome", Method::Codes, kind)
}

/// The problems' (path, code), or none.
fn problems(draft: DiscountDraft) -> Vec<(String, &'static str)> {
    match Discount::new(did(), draft) {
        Ok(_) => Vec::new(),
        Err(DiscountError::Invalid(found)) => found.into_iter().map(|p| (p.path, p.code)).collect(),
        Err(other) => panic!("not a rule: {other}"),
    }
}

fn one(path: &str, code: &'static str) -> Vec<(String, &'static str)> {
    vec![(path.to_string(), code)]
}

#[test]
fn a_standard_discount_takes_its_classes_from_its_targets() {
    let d = Discount::new(
        did(),
        draft(standard(vec![
            ten_percent_off_items(),
            Reward {
                target: Target::Shipping { max_amount: None },
                value: Value::Percent(bp(10_000)),
            },
        ])),
    )
    .unwrap();
    assert_eq!(
        d.classes(),
        ClassSet::of(&[Class::Product, Class::Shipping])
    );
}

#[test]
fn buy_x_get_y_and_tiers_are_product_discounts() {
    let kind = Kind::Tiered {
        items: ItemSet::all(),
        measure: Measure::Quantity,
        tiers: vec![Tier {
            threshold: Threshold::Quantity(2),
            value: Value::Percent(bp(500)),
        }],
    };
    assert_eq!(kind.classes(), ClassSet::of(&[Class::Product]));
}

#[test]
fn a_title_is_required() {
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.title = "  ".to_string();
    assert_eq!(problems(d), one("title", "invalid_text"));
}

#[test]
fn an_end_at_its_start_is_refused() {
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.starts_at = Some(Timestamp::from_unix_micros(10));
    d.ends_at = Some(Timestamp::from_unix_micros(10));
    assert_eq!(problems(d), one("ends_at", "ends_not_after_start"));
}

#[test]
fn a_total_limit_of_zero_is_refused() {
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.limits.total = Some(0);
    assert_eq!(problems(d), one("limits.total", "too_small"));
}

#[test]
fn no_rewards_is_refused() {
    assert_eq!(
        problems(draft(standard(vec![]))),
        one("kind.rewards", "out_of_range")
    );
}

#[test]
fn two_rewards_of_one_class_are_refused() {
    let d = draft(standard(vec![
        ten_percent_off_items(),
        ten_percent_off_items(),
    ]));
    assert_eq!(
        problems(d),
        one("kind.rewards[1].target", "duplicate_class")
    );
}

#[test]
fn a_percentage_over_everything_is_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Subtotal,
        value: Value::Percent(bp(10_001)),
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].value.basis_points", "out_of_range")
    );
}

#[test]
fn a_percentage_of_zero_is_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Subtotal,
        value: Value::Percent(bp(0)),
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].value.basis_points", "out_of_range")
    );
}

#[test]
fn a_whole_percentage_is_allowed() {
    let d = draft(standard(vec![Reward {
        target: Target::Subtotal,
        value: Value::Percent(bp(10_000)),
    }]));
    assert_eq!(problems(d), vec![]);
}

#[test]
fn an_amount_off_each_order_is_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Subtotal,
        value: Value::Amount {
            amounts: PerMarket::of(mkt(1), eur(500)),
            allocation: Allocation::Each,
        },
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].value.allocation", "allocation_not_allowed")
    );
}

#[test]
fn a_fixed_price_for_shipping_is_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Shipping { max_amount: None },
        value: Value::FixedPrice(PerMarket::of(mkt(1), eur(0))),
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].value.type", "value_not_allowed")
    );
}

#[test]
fn a_fixed_price_of_zero_makes_items_free() {
    let d = draft(standard(vec![Reward {
        target: Target::Items(ItemSet::all()),
        value: Value::FixedPrice(PerMarket::of(mkt(1), eur(0))),
    }]));
    assert_eq!(problems(d), vec![]);
}

#[test]
fn an_amount_of_zero_is_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Items(ItemSet::all()),
        value: Value::Amount {
            amounts: PerMarket::of(mkt(1), eur(0)),
            allocation: Allocation::Each,
        },
    }]));
    let path = format!("kind.rewards[0].value.amounts.{}", mkt(1));
    assert_eq!(problems(d), one(&path, "too_small"));
}

#[test]
fn an_amount_needs_a_market() {
    let d = draft(standard(vec![Reward {
        target: Target::Items(ItemSet::all()),
        value: Value::Amount {
            amounts: PerMarket::default(),
            allocation: Allocation::Across,
        },
    }]));
    assert_eq!(problems(d), one("kind.rewards[0].value.amounts", "empty"));
}

#[test]
fn the_cheapest_zero_units_are_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Items(ItemSet::all()),
        value: Value::Amount {
            amounts: PerMarket::of(mkt(1), eur(100)),
            allocation: Allocation::Cheapest { units: 0 },
        },
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].value.allocation.units", "too_small")
    );
}

#[test]
fn an_empty_product_list_is_refused() {
    let d = draft(standard(vec![Reward {
        target: Target::Items(ItemSet {
            items: Items::Products(vec![]),
            exclude_on_sale: false,
        }),
        value: Value::Percent(bp(1000)),
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].target.items.ids", "empty")
    );
}

#[test]
fn a_product_listed_twice_is_refused() {
    let p = ProductId::from_uuid(Uuid::from_u128(5));
    let d = draft(standard(vec![Reward {
        target: Target::Items(ItemSet {
            items: Items::Products(vec![p, p]),
            exclude_on_sale: false,
        }),
        value: Value::Percent(bp(1000)),
    }]));
    assert_eq!(
        problems(d),
        one("kind.rewards[0].target.items.ids[1]", "duplicate")
    );
}

#[test]
fn a_list_of_the_most_ids_is_allowed_and_one_more_refused() {
    let ids = |n: u128| {
        (0..n)
            .map(|i| CustomerId::from_uuid(Uuid::from_u128(i)))
            .collect()
    };
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.conditions = vec![Condition::Customers(ids(MAX_IDS as u128))];
    assert_eq!(problems(d.clone()), vec![]);
    d.conditions = vec![Condition::Customers(ids(MAX_IDS as u128 + 1))];
    assert_eq!(problems(d), one("conditions[0].customers", "too_many"));
}

#[test]
fn a_condition_twice_is_refused() {
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.conditions = vec![Condition::SignedIn, Condition::SignedIn];
    assert_eq!(problems(d), one("conditions[1]", "duplicate"));
}

#[test]
fn a_minimum_quantity_of_zero_is_refused() {
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.conditions = vec![Condition::MinQuantity(0)];
    assert_eq!(problems(d), one("conditions[0].quantity", "too_small"));
}

#[test]
fn every_broken_rule_is_reported_at_once() {
    let mut d = draft(standard(vec![]));
    d.title = String::new();
    d.limits.total = Some(0);
    assert_eq!(problems(d).len(), 3);
}

fn deal(value: Value) -> Kind {
    Kind::BuyXGetY {
        buys: ItemSet::all(),
        buy: Threshold::Quantity(2),
        gets: ItemSet::all(),
        get_quantity: 1,
        value,
        uses_per_order: None,
    }
}

#[test]
fn buy_x_get_y_takes_an_amount_off_each_unit_only() {
    let d = draft(deal(Value::Amount {
        amounts: PerMarket::of(mkt(1), eur(100)),
        allocation: Allocation::Across,
    }));
    assert_eq!(
        problems(d),
        one("kind.value.allocation", "allocation_not_allowed")
    );
}

#[test]
fn buy_x_get_y_gets_at_least_one() {
    let mut kind = deal(Value::Percent(bp(10_000)));
    if let Kind::BuyXGetY { get_quantity, .. } = &mut kind {
        *get_quantity = 0;
    }
    assert_eq!(problems(draft(kind)), one("kind.get_quantity", "too_small"));
}

#[test]
fn buy_x_get_y_buys_at_least_one() {
    let mut kind = deal(Value::Percent(bp(10_000)));
    if let Kind::BuyXGetY { buy, .. } = &mut kind {
        *buy = Threshold::Quantity(0);
    }
    assert_eq!(problems(draft(kind)), one("kind.buy.quantity", "too_small"));
}

#[test]
fn buy_x_get_y_uses_per_order_of_zero_is_refused() {
    let mut kind = deal(Value::Percent(bp(10_000)));
    if let Kind::BuyXGetY { uses_per_order, .. } = &mut kind {
        *uses_per_order = Some(0);
    }
    assert_eq!(
        problems(draft(kind)),
        one("kind.uses_per_order", "too_small")
    );
}

fn tiered(measure: Measure, tiers: Vec<Tier>) -> DiscountDraft {
    draft(Kind::Tiered {
        items: ItemSet::all(),
        measure,
        tiers,
    })
}

fn tier(threshold: Threshold, percent: i32) -> Tier {
    Tier {
        threshold,
        value: Value::Percent(bp(percent)),
    }
}

#[test]
fn tiers_rise() {
    let d = tiered(
        Measure::Quantity,
        vec![
            tier(Threshold::Quantity(5), 1000),
            tier(Threshold::Quantity(5), 1500),
        ],
    );
    assert_eq!(problems(d), one("kind.tiers[1].threshold", "not_ascending"));
}

#[test]
fn a_tier_measured_by_quantity_takes_a_quantity() {
    let d = tiered(
        Measure::Quantity,
        vec![tier(
            Threshold::Amount(PerMarket::of(mkt(1), eur(100))),
            1000,
        )],
    );
    assert_eq!(
        problems(d),
        one("kind.tiers[0].threshold", "threshold_mismatch")
    );
}

#[test]
fn spend_tiers_name_the_same_markets() {
    let d = tiered(
        Measure::Subtotal,
        vec![
            tier(Threshold::Amount(PerMarket::of(mkt(1), eur(100))), 1000),
            tier(Threshold::Amount(PerMarket::of(mkt(2), eur(200))), 1500),
        ],
    );
    assert_eq!(
        problems(d),
        one("kind.tiers[1].threshold.amounts", "markets_differ")
    );
}

#[test]
fn more_tiers_than_the_most_are_refused() {
    let tiers = (1..=MAX_TIERS as u32 + 1)
        .map(|n| tier(Threshold::Quantity(n), 100))
        .collect();
    assert_eq!(
        problems(tiered(Measure::Quantity, tiers)),
        one("kind.tiers", "out_of_range")
    );
}

#[test]
fn an_amount_in_another_currency_than_its_markets_is_refused() {
    let d = Discount::new(
        did(),
        draft(standard(vec![Reward {
            target: Target::Subtotal,
            value: Value::Amount {
                amounts: PerMarket::of(mkt(1), eur(500)),
                allocation: Allocation::Across,
            },
        }])),
    )
    .unwrap();
    assert_eq!(
        d.check_currencies(|_| Some(Currency::USD)),
        Err(DiscountError::CurrencyMismatch {
            path: "kind.rewards[0].value.amounts".to_string(),
            market: mkt(1),
            expected: Currency::USD,
            found: Currency::EUR,
        })
    );
}

#[test]
fn an_amount_for_an_unknown_market_is_refused() {
    let mut d = draft(standard(vec![ten_percent_off_items()]));
    d.conditions = vec![Condition::MinSubtotal(PerMarket::of(mkt(1), eur(500)))];
    let d = Discount::new(did(), d).unwrap();
    assert_eq!(
        d.check_currencies(|_| None),
        Err(DiscountError::MarketNotFound {
            path: "conditions[0].amounts".to_string(),
            market: mkt(1),
        })
    );
}

#[test]
fn the_method_is_fixed() {
    let mut d = Discount::new(did(), draft(standard(vec![ten_percent_off_items()]))).unwrap();
    let mut changed = d.draft();
    changed.method = Method::Automatic;
    assert_eq!(d.update(changed), Err(DiscountError::MethodFixed));
}

#[test]
fn an_update_lists_what_changed() {
    let mut d = Discount::new(did(), draft(standard(vec![ten_percent_off_items()]))).unwrap();
    d.take_events();
    let mut changed = d.draft();
    changed.title = "New".to_string();
    changed.limits.once_per_customer = true;
    d.update(changed).unwrap();
    let events = d.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "discount.updated");
    assert_eq!(
        events[0].data,
        serde_json::json!({"discount": did(), "fields": ["limits", "title"]})
    );
}

#[test]
fn an_unchanged_update_records_nothing() {
    let mut d = Discount::new(did(), draft(standard(vec![ten_percent_off_items()]))).unwrap();
    d.take_events();
    d.update(d.draft()).unwrap();
    assert!(!d.is_changed());
}

#[test]
fn phases_follow_status_and_window() {
    let mut x = draft(standard(vec![ten_percent_off_items()]));
    x.starts_at = Some(Timestamp::from_unix_micros(10));
    x.ends_at = Some(Timestamp::from_unix_micros(20));
    let d = Discount::new(did(), x.clone()).unwrap();
    assert_eq!(d.phase(Timestamp::from_unix_micros(9)), Phase::Scheduled);
    assert_eq!(d.phase(Timestamp::from_unix_micros(10)), Phase::Active);
    assert_eq!(d.phase(Timestamp::from_unix_micros(20)), Phase::Expired);
    x.status = Status::Draft;
    let d = Discount::new(did(), x).unwrap();
    assert_eq!(d.phase(Timestamp::from_unix_micros(15)), Phase::Draft);
}

#[test]
fn combining_needs_each_to_allow_the_others_classes() {
    let mut a = draft(standard(vec![ten_percent_off_items()]));
    a.combines_with = ClassSet::of(&[Class::Order]);
    let a = Discount::new(did(), a).unwrap();
    let mut b = draft(standard(vec![Reward {
        target: Target::Subtotal,
        value: Value::Percent(bp(1000)),
    }]));
    b.combines_with = ClassSet::of(&[Class::Product]);
    let b = Discount::new(DiscountId::from_uuid(Uuid::from_u128(2)), b).unwrap();
    assert!(a.combines(&b));
    let mut c = b.draft();
    c.combines_with = ClassSet::NONE;
    let c = Discount::new(DiscountId::from_uuid(Uuid::from_u128(3)), c).unwrap();
    assert!(!a.combines(&c));
}
