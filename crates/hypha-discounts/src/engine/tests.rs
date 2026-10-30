use std::collections::HashSet;

use hypha_core::{
    BasisPoints, CategoryId, ChannelId, Currency, CustomerGroupId, CustomerId, DiscountId,
    MarketId, Money, ProductId, Timestamp, VariantId,
};
use uuid::Uuid;

use super::*;
use crate::discount::{
    Allocation, ClassSet, Condition, Discount, DiscountDraft, ItemSet, Items, Kind, Limits,
    Measure, Method, PerMarket, Reward, Status, Target, Threshold, Tier, Value,
};

fn did(n: u128) -> DiscountId {
    DiscountId::from_uuid(Uuid::from_u128(n))
}

fn mkt() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(900))
}

fn other_market() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(901))
}

fn chan() -> ChannelId {
    ChannelId::from_uuid(Uuid::from_u128(800))
}

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(100 + n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(200 + n))
}

fn cat(n: u128) -> CategoryId {
    CategoryId::from_uuid(Uuid::from_u128(300 + n))
}

fn eur(n: i64) -> Money {
    Money::new(n, Currency::EUR).unwrap()
}

fn bp(n: i32) -> BasisPoints {
    BasisPoints::new(n).unwrap()
}

fn here(n: i64) -> PerMarket {
    PerMarket::of(mkt(), eur(n))
}

/// Product `n`, variant `n`, `quantity` units at `unit`.
fn line(n: u128, unit: i64, quantity: u32) -> Line {
    Line {
        product: pid(n),
        variant: vid(n),
        categories: HashSet::new(),
        unit_price: eur(unit),
        quantity,
        on_sale: false,
    }
}

fn input(lines: Vec<Line>) -> DiscountInput<'static> {
    DiscountInput {
        at: Timestamp::from_unix_micros(1_000),
        market: mkt(),
        channel: chan(),
        currency: Currency::EUR,
        buyer: Buyer::default(),
        lines,
        shipping: Vec::new(),
        codes: Vec::new(),
        names: crate::Names::none(),
    }
}

fn draft(kind: Kind, combines: ClassSet) -> DiscountDraft {
    let mut d = DiscountDraft::new("Offer", Method::Automatic, kind);
    d.combines_with = combines;
    d
}

fn discount(n: u128, kind: Kind, combines: ClassSet) -> Discount {
    Discount::new(did(n), draft(kind, combines)).unwrap()
}

fn coded(n: u128, kind: Kind) -> Discount {
    let mut d = draft(kind, ClassSet::NONE);
    d.method = Method::Codes;
    Discount::new(did(n), d).unwrap()
}

fn auto(d: &Discount) -> Live<'_> {
    Live {
        discount: d,
        code: None,
        limit_reached: false,
    }
}

fn by_code<'a>(d: &'a Discount, code: &'a str) -> Live<'a> {
    Live {
        discount: d,
        code: Some(code),
        limit_reached: false,
    }
}

fn reward(target: Target, value: Value) -> Kind {
    Kind::Standard {
        rewards: vec![Reward { target, value }],
    }
}

fn items(set: ItemSet, value: Value) -> Kind {
    reward(Target::Items(set), value)
}

fn percent_off_all(n: i32) -> Kind {
    items(ItemSet::all(), Value::Percent(bp(n)))
}

fn amount(n: i64, allocation: Allocation) -> Value {
    Value::Amount {
        amounts: here(n),
        allocation,
    }
}

fn subtotal(value: Value) -> Kind {
    reward(Target::Subtotal, value)
}

fn free_shipping() -> Kind {
    reward(
        Target::Shipping { max_amount: None },
        Value::Percent(bp(10_000)),
    )
}

/// Each line's shares as (application, amount).
fn shares(outcome: &Outcome) -> Vec<Vec<(usize, i64)>> {
    outcome
        .lines
        .iter()
        .map(|l| {
            l.iter()
                .map(|s| (s.application, s.amount.amount()))
                .collect()
        })
        .collect()
}

/// The applications as (discount, amount).
fn applied(outcome: &Outcome) -> Vec<(DiscountId, i64)> {
    outcome
        .applications
        .iter()
        .map(|a| (a.discount, a.amount.amount()))
        .collect()
}

fn states(outcome: &Outcome) -> Vec<CodeState> {
    outcome.codes.iter().map(|c| c.state).collect()
}

// ---- spread ----

#[test]
fn a_cent_over_three_equal_lines_goes_to_the_first() {
    assert_eq!(spread(1, &[100, 100, 100]), vec![1, 0, 0]);
}

#[test]
fn leftover_units_go_to_the_largest_fractions() {
    // 10 over 10:20:30 is 1.67, 3.33, 5: the 0.67 gets the unit left.
    assert_eq!(spread(10, &[10, 20, 30]), vec![2, 3, 5]);
}

#[test]
fn a_half_unit_tie_goes_to_the_earlier_line() {
    assert_eq!(spread(1, &[1, 1]), vec![1, 0]);
}

#[test]
fn a_zero_weight_gets_no_share() {
    assert_eq!(spread(3, &[0, 1, 2]), vec![0, 1, 2]);
}

#[test]
fn an_amount_beyond_the_pool_is_capped_at_it() {
    assert_eq!(spread(1_000, &[100, 200]), vec![100, 200]);
}

#[test]
fn nothing_is_shared_without_weights() {
    assert_eq!(spread(5, &[0, 0]), vec![0, 0]);
}

// ---- product rewards ----

#[test]
fn a_percentage_rounds_a_half_cent_away_from_zero() {
    let d = discount(1, percent_off_all(1000), ClassSet::NONE);
    let outcome = apply(&input(vec![line(1, 1005, 1)]), &[auto(&d)]);
    // 10% of 10.05 is 1.005.
    assert_eq!(shares(&outcome), vec![vec![(0, 101)]]);
}

#[test]
fn a_percentage_below_a_half_cent_rounds_down() {
    let d = discount(1, percent_off_all(1000), ClassSet::NONE);
    let outcome = apply(&input(vec![line(1, 1004, 1)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 100)]]);
}

#[test]
fn a_percentage_rounds_once_per_line_not_per_unit() {
    let d = discount(1, percent_off_all(1000), ClassSet::NONE);
    // 3 × 0.15 = 0.45: 10% is 0.045, so 5 cents for the line (per unit it'd be 3 × 2).
    let outcome = apply(&input(vec![line(1, 15, 3)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 5)]]);
}

#[test]
fn combined_percentages_add_up_on_the_same_base() {
    let ten = discount(1, percent_off_all(1000), ClassSet::ALL);
    let twenty = discount(2, percent_off_all(2000), ClassSet::ALL);
    let outcome = apply(
        &input(vec![line(1, 10_000, 1)]),
        &[auto(&ten), auto(&twenty)],
    );
    assert_eq!(applied(&outcome), vec![(did(1), 1000), (did(2), 2000)]);
    assert_eq!(outcome.total, eur(3000));
}

#[test]
fn rewards_past_a_lines_base_are_clipped_so_it_reaches_zero() {
    let a = discount(1, percent_off_all(6000), ClassSet::ALL);
    let b = discount(2, percent_off_all(6000), ClassSet::ALL);
    let outcome = apply(&input(vec![line(1, 1000, 1)]), &[auto(&a), auto(&b)]);
    assert_eq!(applied(&outcome), vec![(did(1), 600), (did(2), 400)]);
}

#[test]
fn clipping_takes_percentages_before_amounts_whatever_the_ids() {
    let amount_first = discount(
        1,
        items(ItemSet::all(), amount(800, Allocation::Each)),
        ClassSet::ALL,
    );
    let half = discount(2, percent_off_all(5000), ClassSet::ALL);
    let outcome = apply(
        &input(vec![line(1, 1000, 1)]),
        &[auto(&amount_first), auto(&half)],
    );
    assert_eq!(applied(&outcome), vec![(did(1), 500), (did(2), 500)]);
}

#[test]
fn clipping_takes_amounts_before_fixed_prices() {
    let fixed = discount(
        1,
        items(ItemSet::all(), Value::FixedPrice(here(100))),
        ClassSet::ALL,
    );
    let off = discount(
        2,
        items(ItemSet::all(), amount(700, Allocation::Each)),
        ClassSet::ALL,
    );
    let outcome = apply(&input(vec![line(1, 1000, 1)]), &[auto(&fixed), auto(&off)]);
    // The amount takes 700 first; the fixed price's 900 gets the 300 left.
    assert_eq!(applied(&outcome), vec![(did(1), 300), (did(2), 700)]);
}

#[test]
fn an_amount_off_each_is_per_unit() {
    let d = discount(
        1,
        items(ItemSet::all(), amount(200, Allocation::Each)),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 500, 3)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 600)]]);
}

#[test]
fn an_amount_off_each_stops_at_the_units_price() {
    let d = discount(
        1,
        items(ItemSet::all(), amount(700, Allocation::Each)),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 500, 3)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 1500)]]);
}

#[test]
fn an_amount_across_is_spread_by_largest_remainder() {
    let d = discount(
        1,
        items(ItemSet::all(), amount(100, Allocation::Across)),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 100, 1), line(2, 100, 1), line(3, 100, 1)]),
        &[auto(&d)],
    );
    assert_eq!(
        shares(&outcome),
        vec![vec![(0, 34)], vec![(0, 33)], vec![(0, 33)]]
    );
}

#[test]
fn an_amount_across_larger_than_the_items_is_capped_at_them() {
    let d = discount(
        1,
        items(ItemSet::all(), amount(1_000, Allocation::Across)),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 100, 1), line(2, 200, 1)]), &[auto(&d)]);
    assert_eq!(outcome.total, eur(300));
}

#[test]
fn cheapest_covers_the_lowest_priced_units_first() {
    let d = discount(
        1,
        items(
            ItemSet::all(),
            amount(1_000, Allocation::Cheapest { units: 2 }),
        ),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 1000, 2), line(2, 300, 1), line(3, 500, 3)]),
        &[auto(&d)],
    );
    assert_eq!(
        shares(&outcome),
        vec![vec![], vec![(0, 300)], vec![(0, 500)]]
    );
}

#[test]
fn cheapest_ties_go_to_the_earlier_line() {
    let d = discount(
        1,
        items(
            ItemSet::all(),
            amount(100, Allocation::Cheapest { units: 1 }),
        ),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 500, 1), line(2, 500, 1)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 100)], vec![]]);
}

#[test]
fn a_fixed_price_reduces_only_units_above_it() {
    let d = discount(
        1,
        items(ItemSet::all(), Value::FixedPrice(here(500))),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 1000, 2), line(2, 400, 1)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 1000)], vec![]]);
}

#[test]
fn excluding_sale_items_leaves_lines_on_sale_alone() {
    let d = discount(
        1,
        items(
            ItemSet {
                items: Items::All,
                exclude_on_sale: true,
            },
            Value::Percent(bp(1000)),
        ),
        ClassSet::NONE,
    );
    let mut sale = line(1, 1000, 1);
    sale.on_sale = true;
    let outcome = apply(&input(vec![sale, line(2, 1000, 1)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![], vec![(0, 100)]]);
}

#[test]
fn a_category_reaches_products_in_categories_below_it() {
    let d = discount(
        1,
        items(
            ItemSet {
                items: Items::Categories(vec![cat(1)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(1000)),
        ),
        ClassSet::NONE,
    );
    let mut child = line(1, 1000, 1);
    // The cart gives a product's categories with their ancestors.
    child.categories = HashSet::from([cat(2), cat(1)]);
    let outcome = apply(&input(vec![child, line(2, 1000, 1)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![(0, 100)], vec![]]);
}

#[test]
fn listed_variants_are_the_only_ones_reduced() {
    let d = discount(
        1,
        items(
            ItemSet {
                items: Items::Variants(vec![vid(2)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(1000)),
        ),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 1000, 1)]),
        &[auto(&d)],
    );
    assert_eq!(shares(&outcome), vec![vec![], vec![(0, 100)]]);
}

#[test]
fn listed_products_are_the_only_ones_reduced() {
    let d = discount(
        1,
        items(
            ItemSet {
                items: Items::Products(vec![pid(1)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(1000)),
        ),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 1000, 1)]),
        &[auto(&d)],
    );
    assert_eq!(shares(&outcome), vec![vec![(0, 100)], vec![]]);
}

// ---- order rewards ----

#[test]
fn an_order_percentage_is_on_the_subtotal_after_product_discounts() {
    let product = discount(
        1,
        items(
            ItemSet {
                items: Items::Products(vec![pid(1)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(1000)),
        ),
        ClassSet::ALL,
    );
    let order = discount(2, subtotal(Value::Percent(bp(1000))), ClassSet::ALL);
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 1000, 1)]),
        &[auto(&product), auto(&order)],
    );
    // 10% of 900 + 1000 = 190, spread 90 : 100.
    assert_eq!(applied(&outcome), vec![(did(1), 100), (did(2), 190)]);
    assert_eq!(
        shares(&outcome),
        vec![vec![(0, 100), (1, 90)], vec![(1, 100)]]
    );
}

#[test]
fn order_percentages_add_up_on_the_same_subtotal() {
    let a = discount(1, subtotal(Value::Percent(bp(1000))), ClassSet::ALL);
    let b = discount(2, subtotal(Value::Percent(bp(2000))), ClassSet::ALL);
    let outcome = apply(&input(vec![line(1, 10_000, 1)]), &[auto(&a), auto(&b)]);
    assert_eq!(applied(&outcome), vec![(did(1), 1000), (did(2), 2000)]);
}

#[test]
fn order_amounts_past_the_subtotal_are_clipped_after_percentages() {
    let off = discount(1, subtotal(amount(800, Allocation::Across)), ClassSet::ALL);
    let half = discount(2, subtotal(Value::Percent(bp(5000))), ClassSet::ALL);
    let outcome = apply(&input(vec![line(1, 1000, 1)]), &[auto(&off), auto(&half)]);
    assert_eq!(applied(&outcome), vec![(did(1), 500), (did(2), 500)]);
}

#[test]
fn several_order_rewards_never_take_a_line_below_zero() {
    let a = discount(1, subtotal(amount(1, Allocation::Across)), ClassSet::ALL);
    let b = discount(2, subtotal(amount(1, Allocation::Across)), ClassSet::ALL);
    let outcome = apply(
        &input(vec![line(1, 1, 1), line(2, 1, 1)]),
        &[auto(&a), auto(&b)],
    );
    // The second spreads over what the first left: one cent each.
    assert_eq!(shares(&outcome), vec![vec![(0, 1)], vec![(1, 1)]]);
}

#[test]
fn an_order_discount_skips_a_line_that_costs_nothing() {
    let d = discount(1, subtotal(amount(10, Allocation::Across)), ClassSet::NONE);
    let outcome = apply(&input(vec![line(1, 0, 1), line(2, 100, 1)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![], vec![(0, 10)]]);
}

#[test]
fn an_order_minimum_is_measured_after_product_discounts() {
    let product = discount(1, percent_off_all(1000), ClassSet::ALL);
    let mut order = draft(subtotal(amount(500, Allocation::Across)), ClassSet::ALL);
    order.conditions = vec![Condition::MinSubtotal(here(10_000))];
    let order = Discount::new(did(2), order).unwrap();
    let outcome = apply(
        &input(vec![line(1, 10_000, 1)]),
        &[auto(&product), auto(&order)],
    );
    // 10,000 less 10% is 9,000: under the minimum.
    assert_eq!(applied(&outcome), vec![(did(1), 1000)]);
}

// ---- shipping ----

fn shipped<'a>(lines: Vec<Line>, rate: &'a dyn Fn(Money) -> Option<Money>) -> DiscountInput<'a> {
    let indexes = (0..lines.len()).collect();
    DiscountInput {
        shipping: vec![ShippingGroup {
            lines: indexes,
            rate,
        }],
        ..input(lines)
    }
}

#[test]
fn free_shipping_takes_the_whole_rate() {
    let rate = |_: Money| Some(eur(500));
    let d = discount(1, free_shipping(), ClassSet::NONE);
    let outcome = apply(&shipped(vec![line(1, 1000, 1)], &rate), &[auto(&d)]);
    assert_eq!(outcome.shipping[0].amount, Some(eur(500)));
    assert_eq!(outcome.shipping[0].shares[0].amount, eur(500));
}

#[test]
fn a_shipping_cap_limits_what_comes_off() {
    let rate = |_: Money| Some(eur(1500));
    let d = discount(
        1,
        reward(
            Target::Shipping {
                max_amount: Some(here(1000)),
            },
            Value::Percent(bp(10_000)),
        ),
        ClassSet::NONE,
    );
    let outcome = apply(&shipped(vec![line(1, 1000, 1)], &rate), &[auto(&d)]);
    assert_eq!(outcome.total, eur(1000));
}

#[test]
fn a_rate_sees_the_subtotal_after_discounts() {
    // Free over 50.00: a 20% order discount takes 60.00 under it (and still saves:
    // 48.00 + 5.00).
    let rate = |subtotal: Money| Some(eur(if subtotal.amount() >= 5000 { 0 } else { 500 }));
    let d = discount(1, subtotal(Value::Percent(bp(2000))), ClassSet::NONE);
    let outcome = apply(&shipped(vec![line(1, 6000, 1)], &rate), &[auto(&d)]);
    assert_eq!(outcome.shipping[0].subtotal, eur(4800));
    assert_eq!(outcome.shipping[0].amount, Some(eur(500)));
}

/// Free shipping from 50.00, else 5.00.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the shape of a shipping rate function"
)]
fn free_over_fifty(subtotal: Money) -> Option<Money> {
    Some(eur(if subtotal.amount() >= 5000 { 0 } else { 500 }))
}

#[test]
fn no_discount_applies_when_it_would_make_the_cart_cost_more() {
    // 5% off 52.00 takes the cart under free shipping: 49.40 + 5.00 is more than 52.00.
    let d = discount(1, subtotal(Value::Percent(bp(500))), ClassSet::NONE);
    let outcome = apply(
        &shipped(vec![line(1, 5200, 1)], &free_over_fifty),
        &[auto(&d)],
    );
    assert_eq!(applied(&outcome), vec![]);
    assert_eq!(outcome.shipping[0].amount, Some(eur(0)));
    assert_eq!(outcome.shipping[0].subtotal, eur(5200));
}

#[test]
fn a_code_that_would_make_the_cart_cost_more_says_it_saves_nothing() {
    let d = coded(1, subtotal(Value::Percent(bp(500))));
    let mut i = shipped(vec![line(1, 5200, 1)], &free_over_fifty);
    i.codes = vec!["CODE".to_string()];
    let outcome = apply(&i, &[by_code(&d, "CODE")]);
    assert_eq!(states(&outcome), vec![CodeState::NoSaving]);
    assert_eq!(outcome.total, eur(0));
}

#[test]
fn a_discount_that_keeps_free_shipping_still_applies() {
    // 5% off 60.00 leaves 57.00: still free shipping.
    let d = discount(1, subtotal(Value::Percent(bp(500))), ClassSet::NONE);
    let outcome = apply(
        &shipped(vec![line(1, 6000, 1)], &free_over_fifty),
        &[auto(&d)],
    );
    assert_eq!(applied(&outcome), vec![(did(1), 300)]);
}

#[test]
fn without_a_chosen_option_free_shipping_saves_nothing() {
    let rate = |_: Money| None;
    let d = coded(1, free_shipping());
    let mut i = shipped(vec![line(1, 1000, 1)], &rate);
    i.codes = vec!["SHIP".to_string()];
    let outcome = apply(&i, &[by_code(&d, "SHIP")]);
    assert_eq!(states(&outcome), vec![CodeState::NoMatchingItems]);
}

#[test]
fn one_shipping_discount_applies_however_many_combine() {
    let rate = |_: Money| Some(eur(500));
    let a = discount(1, free_shipping(), ClassSet::ALL);
    let b = discount(2, free_shipping(), ClassSet::ALL);
    let outcome = apply(
        &shipped(vec![line(1, 1000, 1)], &rate),
        &[auto(&a), auto(&b)],
    );
    assert_eq!(applied(&outcome), vec![(did(1), 500)]);
}

// ---- combining ----

#[test]
fn discounts_that_dont_combine_give_way_to_the_bigger_saving() {
    let product = discount(1, percent_off_all(1000), ClassSet::NONE);
    let order = discount(2, subtotal(Value::Percent(bp(1500))), ClassSet::NONE);
    let outcome = apply(
        &input(vec![line(1, 1000, 1)]),
        &[auto(&product), auto(&order)],
    );
    assert_eq!(applied(&outcome), vec![(did(2), 150)]);
}

#[test]
fn combining_needs_both_to_allow_the_others_class() {
    // The product discount combines with orders; the order one with nothing.
    let product = discount(1, percent_off_all(1000), ClassSet::of(&[Class::Order]));
    let order = discount(2, subtotal(Value::Percent(bp(1500))), ClassSet::NONE);
    let outcome = apply(
        &input(vec![line(1, 1000, 1)]),
        &[auto(&product), auto(&order)],
    );
    assert_eq!(applied(&outcome), vec![(did(2), 150)]);
}

#[test]
fn product_discounts_that_dont_combine_still_share_a_cart_on_different_lines() {
    let first = discount(
        1,
        items(
            ItemSet {
                items: Items::Products(vec![pid(1)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(1000)),
        ),
        ClassSet::NONE,
    );
    let second = discount(
        2,
        items(
            ItemSet {
                items: Items::Products(vec![pid(2)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(2000)),
        ),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 1000, 1)]),
        &[auto(&first), auto(&second)],
    );
    assert_eq!(applied(&outcome), vec![(did(1), 100), (did(2), 200)]);
}

#[test]
fn an_equal_saving_goes_to_fewer_discounts() {
    let order = discount(1, subtotal(amount(300, Allocation::Across)), ClassSet::NONE);
    let only = |n: u128| ItemSet {
        items: Items::Products(vec![pid(n)]),
        exclude_on_sale: false,
    };
    let a = discount(
        2,
        items(only(1), amount(150, Allocation::Across)),
        ClassSet::of(&[Class::Product]),
    );
    let b = discount(
        3,
        items(only(2), amount(150, Allocation::Across)),
        ClassSet::of(&[Class::Product]),
    );
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 1000, 1)]),
        &[auto(&a), auto(&b), auto(&order)],
    );
    assert_eq!(applied(&outcome), vec![(did(1), 300)]);
}

#[test]
fn an_equal_saving_with_as_many_discounts_goes_to_the_lower_id() {
    let a = discount(2, percent_off_all(1000), ClassSet::NONE);
    let b = discount(1, percent_off_all(1000), ClassSet::NONE);
    let outcome = apply(&input(vec![line(1, 1000, 1)]), &[auto(&a), auto(&b)]);
    assert_eq!(applied(&outcome), vec![(did(1), 100)]);
}

#[test]
fn every_seed_is_tried_so_a_pair_can_beat_the_biggest_alone() {
    // The biggest (order 25%) combines with nothing; two product discounts that combine
    // with each other save more together.
    let big = discount(1, subtotal(Value::Percent(bp(2500))), ClassSet::NONE);
    let a = discount(2, percent_off_all(2000), ClassSet::of(&[Class::Product]));
    let b = discount(3, percent_off_all(1500), ClassSet::of(&[Class::Product]));
    let outcome = apply(
        &input(vec![line(1, 1000, 1)]),
        &[auto(&big), auto(&a), auto(&b)],
    );
    assert_eq!(applied(&outcome), vec![(did(2), 200), (did(3), 150)]);
}

#[test]
fn applications_come_by_class_then_id_and_shares_name_them() {
    let rate = |_: Money| Some(eur(500));
    let all = ClassSet::ALL;
    let ship = discount(1, free_shipping(), all);
    let order = discount(2, subtotal(Value::Percent(bp(1000))), all);
    let product = discount(3, percent_off_all(1000), all);
    let outcome = apply(
        &shipped(vec![line(1, 1000, 1)], &rate),
        &[auto(&ship), auto(&order), auto(&product)],
    );
    let classes: Vec<Class> = outcome.applications.iter().map(|a| a.class).collect();
    assert_eq!(classes, vec![Class::Product, Class::Order, Class::Shipping]);
    assert_eq!(shares(&outcome), vec![vec![(0, 100), (1, 90)]]);
    assert_eq!(outcome.shipping[0].shares[0].application, 2);
}

#[test]
fn a_discount_with_two_rewards_applies_as_two() {
    let rate = |_: Money| Some(eur(500));
    let d = discount(
        1,
        Kind::Standard {
            rewards: vec![
                Reward {
                    target: Target::Items(ItemSet::all()),
                    value: Value::Percent(bp(2000)),
                },
                Reward {
                    target: Target::Shipping { max_amount: None },
                    value: Value::Percent(bp(10_000)),
                },
            ],
        },
        ClassSet::NONE,
    );
    let outcome = apply(&shipped(vec![line(1, 1000, 1)], &rate), &[auto(&d)]);
    assert_eq!(applied(&outcome), vec![(did(1), 200), (did(1), 500)]);
}

#[test]
fn every_application_is_the_sum_of_its_shares() {
    let rate = |_: Money| Some(eur(333));
    let all = ClassSet::ALL;
    let d1 = discount(
        1,
        items(ItemSet::all(), amount(101, Allocation::Across)),
        all,
    );
    let d2 = discount(2, subtotal(Value::Percent(bp(1333))), all);
    let d3 = discount(
        3,
        reward(
            Target::Shipping { max_amount: None },
            Value::Percent(bp(3333)),
        ),
        all,
    );
    let outcome = apply(
        &shipped(vec![line(1, 333, 3), line(2, 777, 1), line(3, 1, 7)], &rate),
        &[auto(&d1), auto(&d2), auto(&d3)],
    );
    for (i, a) in outcome.applications.iter().enumerate() {
        let on_lines: i64 = outcome
            .lines
            .iter()
            .flatten()
            .filter(|s| s.application == i)
            .map(|s| s.amount.amount())
            .sum();
        let on_shipping: i64 = outcome
            .shipping
            .iter()
            .flat_map(|g| &g.shares)
            .filter(|s| s.application == i)
            .map(|s| s.amount.amount())
            .sum();
        assert_eq!(a.amount.amount(), on_lines + on_shipping);
    }
    let sum: i64 = outcome.applications.iter().map(|a| a.amount.amount()).sum();
    assert_eq!(outcome.total, eur(sum));
}

// ---- code states ----

fn with_code(lines: Vec<Line>) -> DiscountInput<'static> {
    DiscountInput {
        codes: vec!["CODE".to_string()],
        ..input(lines)
    }
}

fn state_of(d: &Discount, i: &DiscountInput<'_>) -> CodeState {
    states(&apply(i, &[by_code(d, "CODE")]))[0]
}

#[test]
fn a_code_in_the_winning_set_is_applied() {
    let d = coded(1, percent_off_all(1000));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::Applied
    );
}

#[test]
fn a_code_nothing_matches_says_so() {
    let d = coded(
        1,
        items(
            ItemSet {
                items: Items::Products(vec![pid(9)]),
                exclude_on_sale: false,
            },
            Value::Percent(bp(1000)),
        ),
    );
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::NoMatchingItems
    );
}

#[test]
fn an_unknown_code_is_invalid() {
    assert_eq!(
        states(&apply(&with_code(vec![line(1, 1000, 1)]), &[])),
        vec![CodeState::Invalid]
    );
}

#[test]
fn a_code_whose_discount_ended_is_expired() {
    let mut draft = draft(percent_off_all(1000), ClassSet::NONE);
    draft.method = Method::Codes;
    draft.ends_at = Some(Timestamp::from_unix_micros(500));
    let d = Discount::new(did(1), draft).unwrap();
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::Expired
    );
}

#[test]
fn a_code_whose_discount_is_a_draft_is_invalid() {
    let mut draft = draft(percent_off_all(1000), ClassSet::NONE);
    draft.method = Method::Codes;
    draft.status = Status::Draft;
    let d = Discount::new(did(1), draft).unwrap();
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::Invalid
    );
}

#[test]
fn a_code_whose_discount_hasnt_started_is_invalid() {
    let mut draft = draft(percent_off_all(1000), ClassSet::NONE);
    draft.method = Method::Codes;
    draft.starts_at = Some(Timestamp::from_unix_micros(5_000));
    let d = Discount::new(did(1), draft).unwrap();
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::Invalid
    );
}

fn conditioned(condition: Condition) -> Discount {
    let mut draft = draft(percent_off_all(1000), ClassSet::NONE);
    draft.method = Method::Codes;
    draft.conditions = vec![condition];
    Discount::new(did(1), draft).unwrap()
}

#[test]
fn a_code_for_another_market_names_the_condition() {
    let d = conditioned(Condition::Markets(vec![other_market()]));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::NotEligible {
            condition: "markets"
        }
    );
}

#[test]
fn a_code_for_another_channel_names_the_condition() {
    let d = conditioned(Condition::Channels(vec![ChannelId::from_uuid(
        Uuid::from_u128(1),
    )]));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::NotEligible {
            condition: "channels"
        }
    );
}

#[test]
fn a_code_for_a_group_the_buyer_is_in_applies() {
    let group = CustomerGroupId::from_uuid(Uuid::from_u128(1));
    let d = conditioned(Condition::CustomerGroups(vec![group]));
    let mut i = with_code(vec![line(1, 1000, 1)]);
    assert_eq!(
        state_of(&d, &i),
        CodeState::NotEligible {
            condition: "customer_groups"
        }
    );
    i.buyer.groups = vec![group];
    assert_eq!(state_of(&d, &i), CodeState::Applied);
}

#[test]
fn a_code_for_listed_customers_applies_only_to_them() {
    let customer = CustomerId::from_uuid(Uuid::from_u128(1));
    let d = conditioned(Condition::Customers(vec![customer]));
    let mut i = with_code(vec![line(1, 1000, 1)]);
    assert_eq!(
        state_of(&d, &i),
        CodeState::NotEligible {
            condition: "customers"
        }
    );
    i.buyer.customer = Some(customer);
    i.buyer.signed_in = true;
    assert_eq!(state_of(&d, &i), CodeState::Applied);
}

#[test]
fn a_guest_with_a_listed_customers_email_isnt_that_customer() {
    // A guest's email finds its customer only to count their uses.
    let customer = CustomerId::from_uuid(Uuid::from_u128(1));
    let d = conditioned(Condition::Customers(vec![customer]));
    let mut i = with_code(vec![line(1, 1000, 1)]);
    i.buyer.customer = Some(customer);
    assert_eq!(
        state_of(&d, &i),
        CodeState::NotEligible {
            condition: "customers"
        }
    );
}

#[test]
fn a_code_for_signed_in_customers_needs_a_customer() {
    let d = conditioned(Condition::SignedIn);
    let mut i = with_code(vec![line(1, 1000, 1)]);
    assert_eq!(
        state_of(&d, &i),
        CodeState::NotEligible {
            condition: "signed_in"
        }
    );
    i.buyer.signed_in = true;
    assert_eq!(state_of(&d, &i), CodeState::Applied);
}

#[test]
fn a_code_short_of_its_minimum_says_how_much_is_missing() {
    let d = conditioned(Condition::MinSubtotal(here(5000)));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 3)])),
        CodeState::MinimumNotMet {
            needed: Needed::Amount(eur(2000))
        }
    );
}

#[test]
fn a_code_at_exactly_its_minimum_applies() {
    let d = conditioned(Condition::MinSubtotal(here(5000)));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 5)])),
        CodeState::Applied
    );
}

#[test]
fn a_code_short_of_its_quantity_says_how_many_are_missing() {
    let d = conditioned(Condition::MinQuantity(3));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::MinimumNotMet {
            needed: Needed::Quantity(2)
        }
    );
}

#[test]
fn a_code_with_its_uses_taken_has_reached_its_limit() {
    let d = coded(1, percent_off_all(1000));
    let live = Live {
        discount: &d,
        code: Some("CODE"),
        limit_reached: true,
    };
    let outcome = apply(&with_code(vec![line(1, 1000, 1)]), &[live]);
    assert_eq!(states(&outcome), vec![CodeState::LimitReached]);
    assert_eq!(outcome.total, eur(0));
}

#[test]
fn a_code_without_an_amount_in_the_market_isnt_available_there() {
    let d = coded(
        1,
        items(
            ItemSet::all(),
            Value::Amount {
                amounts: PerMarket::of(other_market(), eur(100)),
                allocation: Allocation::Each,
            },
        ),
    );
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 1)])),
        CodeState::NotAvailableInMarket
    );
}

#[test]
fn a_code_a_better_combination_leaves_out_names_the_one_that_did() {
    let code = coded(1, percent_off_all(1000));
    let better = discount(2, subtotal(Value::Percent(bp(2000))), ClassSet::NONE);
    let outcome = apply(
        &with_code(vec![line(1, 1000, 1)]),
        &[by_code(&code, "CODE"), auto(&better)],
    );
    assert_eq!(
        states(&outcome),
        vec![CodeState::BlockedByOther { discount: did(2) }]
    );
}

#[test]
fn a_second_code_of_the_same_discount_adds_nothing() {
    let d = coded(1, percent_off_all(1000));
    let mut i = input(vec![line(1, 1000, 1)]);
    i.codes = vec!["ONE".to_string(), "TWO".to_string()];
    let outcome = apply(&i, &[by_code(&d, "ONE"), by_code(&d, "TWO")]);
    assert_eq!(
        states(&outcome),
        vec![
            CodeState::Applied,
            CodeState::BlockedByOther { discount: did(1) }
        ]
    );
    assert_eq!(outcome.total, eur(100));
    assert_eq!(outcome.applications[0].code.as_deref(), Some("ONE"));
}

#[test]
fn of_two_codes_of_one_discount_the_first_entered_applies() {
    let d = coded(1, percent_off_all(1000));
    let mut i = input(vec![line(1, 1000, 1)]);
    i.codes = vec!["ONE".to_string(), "TWO".to_string()];
    // Storage finds them in its own order.
    let outcome = apply(&i, &[by_code(&d, "TWO"), by_code(&d, "ONE")]);
    assert_eq!(outcome.applications[0].code.as_deref(), Some("ONE"));
    assert_eq!(
        states(&outcome),
        vec![
            CodeState::Applied,
            CodeState::BlockedByOther { discount: did(1) }
        ]
    );
}

#[test]
fn a_used_up_code_gives_way_to_another_of_its_discount() {
    let d = coded(1, percent_off_all(1000));
    let mut i = input(vec![line(1, 1000, 1)]);
    i.codes = vec!["ONE".to_string(), "TWO".to_string()];
    let used_up = Live {
        limit_reached: true,
        ..by_code(&d, "ONE")
    };
    let outcome = apply(&i, &[used_up, by_code(&d, "TWO")]);
    assert_eq!(outcome.total, eur(100));
    assert_eq!(outcome.applications[0].code.as_deref(), Some("TWO"));
    assert_eq!(
        states(&outcome),
        vec![CodeState::LimitReached, CodeState::Applied]
    );
}

// ---- buy X get Y ----

fn bxgy(buy: Threshold, get: u32, value: Value, uses: Option<u32>) -> Kind {
    Kind::BuyXGetY {
        buys: ItemSet::all(),
        buy,
        gets: ItemSet::all(),
        get_quantity: get,
        value,
        uses_per_order: uses,
    }
}

fn free() -> Value {
    Value::Percent(bp(10_000))
}

#[test]
fn buy_two_get_one_free_gives_the_cheapest_unit() {
    let d = discount(
        1,
        bxgy(Threshold::Quantity(2), 1, free(), None),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 800, 1), line(3, 500, 1)]),
        &[auto(&d)],
    );
    assert_eq!(shares(&outcome), vec![vec![], vec![], vec![(0, 500)]]);
    assert_eq!(outcome.lines[2][0].units, Some(1));
}

#[test]
fn buy_x_get_y_repeats_while_the_cart_qualifies() {
    let d = discount(
        1,
        bxgy(Threshold::Quantity(1), 1, free(), None),
        ClassSet::NONE,
    );
    // Six units: three bought (most expensive), three got.
    let outcome = apply(&input(vec![line(1, 1000, 3), line(2, 500, 3)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![], vec![(0, 1500)]]);
}

#[test]
fn buy_x_get_y_stops_at_its_uses_per_order() {
    let d = discount(
        1,
        bxgy(Threshold::Quantity(1), 1, free(), Some(2)),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 1000, 3), line(2, 500, 3)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![], vec![(0, 1000)]]);
}

#[test]
fn buy_x_get_y_needs_the_free_unit_in_the_cart() {
    let d = coded(1, bxgy(Threshold::Quantity(2), 1, free(), None));
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 2)])),
        CodeState::NoMatchingItems
    );
}

#[test]
fn buy_x_get_y_by_amount_counts_full_prices() {
    let d = discount(
        1,
        bxgy(Threshold::Amount(here(1500)), 1, free(), None),
        ClassSet::NONE,
    );
    // 1000 + 800 meets 15.00; the 300 is got.
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 800, 1), line(3, 300, 1)]),
        &[auto(&d)],
    );
    assert_eq!(outcome.total, eur(300));
}

#[test]
fn a_percentage_got_rounds_once_per_line() {
    let d = discount(
        1,
        bxgy(Threshold::Quantity(1), 1, Value::Percent(bp(5000)), None),
        ClassSet::NONE,
    );
    // Two units of 1.01 bought, two got: 50% of 2.02 is 1.01, not 2 × 0.505 → 2 × 0.51.
    let outcome = apply(&input(vec![line(1, 2000, 2), line(2, 101, 2)]), &[auto(&d)]);
    assert_eq!(shares(&outcome), vec![vec![], vec![(0, 101)]]);
}

#[test]
fn an_amount_off_each_unit_got_stops_at_its_price() {
    let d = discount(
        1,
        bxgy(
            Threshold::Quantity(1),
            1,
            amount(700, Allocation::Each),
            None,
        ),
        ClassSet::NONE,
    );
    let outcome = apply(&input(vec![line(1, 1000, 1), line(2, 500, 1)]), &[auto(&d)]);
    assert_eq!(outcome.total, eur(500));
}

#[test]
fn a_discount_alone_beats_a_combination_it_saves_less_in() {
    let only = |n: u128| ItemSet {
        items: Items::Products(vec![pid(n)]),
        exclude_on_sale: false,
    };
    let sale = discount(1, items(only(1), Value::Percent(bp(1000))), ClassSet::ALL);
    let deal = discount(
        2,
        Kind::BuyXGetY {
            buys: ItemSet::all(),
            buy: Threshold::Quantity(1),
            gets: only(2),
            get_quantity: 1,
            value: free(),
            uses_per_order: None,
        },
        ClassSet::ALL,
    );
    // A 10.00 shirt on sale and a 5.00 sock got free for buying anything. Together the
    // sale reduces the shirt, so only the sock counts as bought and nothing is left to
    // get: 1.00 off. The deal alone takes 5.00 off, so it wins.
    let outcome = apply(
        &input(vec![line(1, 1000, 1), line(2, 500, 1)]),
        &[auto(&sale), auto(&deal)],
    );
    assert_eq!(applied(&outcome), vec![(did(2), 500)]);
}

#[test]
fn gets_are_taken_from_other_units_than_the_ones_bought() {
    let d = discount(
        1,
        bxgy(Threshold::Quantity(2), 1, free(), None),
        ClassSet::NONE,
    );
    // Three of one variant: two bought, the third got.
    let outcome = apply(&input(vec![line(1, 1000, 3)]), &[auto(&d)]);
    assert_eq!(outcome.total, eur(1000));
    assert_eq!(outcome.lines[0][0].units, Some(1));
}

// ---- tiers ----

fn tiers(measure: Measure, steps: Vec<(Threshold, Value)>) -> Kind {
    Kind::Tiered {
        items: ItemSet::all(),
        measure,
        tiers: steps
            .into_iter()
            .map(|(threshold, value)| Tier { threshold, value })
            .collect(),
    }
}

fn volume() -> Kind {
    tiers(
        Measure::Quantity,
        vec![
            (Threshold::Quantity(3), Value::Percent(bp(1000))),
            (Threshold::Quantity(5), Value::Percent(bp(1500))),
        ],
    )
}

#[test]
fn a_tier_applies_from_exactly_its_threshold() {
    let d = discount(1, volume(), ClassSet::NONE);
    let outcome = apply(&input(vec![line(1, 1000, 3)]), &[auto(&d)]);
    assert_eq!(outcome.total, eur(300));
}

#[test]
fn the_highest_tier_reached_applies() {
    let d = discount(1, volume(), ClassSet::NONE);
    let outcome = apply(&input(vec![line(1, 1000, 6)]), &[auto(&d)]);
    assert_eq!(outcome.total, eur(900));
    assert_eq!(outcome.applications[0].value, Value::Percent(bp(1500)));
}

#[test]
fn short_of_the_first_tier_says_how_many_more() {
    let d = coded(1, volume());
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 1000, 2)])),
        CodeState::MinimumNotMet {
            needed: Needed::Quantity(1)
        }
    );
}

#[test]
fn a_spend_tier_takes_its_amount_across_the_items() {
    let d = discount(
        1,
        tiers(
            Measure::Subtotal,
            vec![(
                Threshold::Amount(here(10_000)),
                amount(1500, Allocation::Across),
            )],
        ),
        ClassSet::NONE,
    );
    let outcome = apply(
        &input(vec![line(1, 6000, 1), line(2, 4000, 1)]),
        &[auto(&d)],
    );
    assert_eq!(shares(&outcome), vec![vec![(0, 900)], vec![(0, 600)]]);
}

#[test]
fn a_spend_tier_below_its_amount_says_how_much_more() {
    let d = coded(
        1,
        tiers(
            Measure::Subtotal,
            vec![(
                Threshold::Amount(here(10_000)),
                amount(1500, Allocation::Across),
            )],
        ),
    );
    assert_eq!(
        state_of(&d, &with_code(vec![line(1, 9_999, 1)])),
        CodeState::MinimumNotMet {
            needed: Needed::Amount(eur(1))
        }
    );
}

#[test]
fn limits_dont_reach_the_engine_except_as_reached() {
    // The engine never counts uses: the cart says whether they're taken.
    let mut d = draft(percent_off_all(1000), ClassSet::NONE);
    d.limits = Limits {
        total: Some(1),
        once_per_customer: true,
    };
    let d = Discount::new(did(1), d).unwrap();
    let outcome = apply(&input(vec![line(1, 1000, 1)]), &[auto(&d)]);
    assert_eq!(outcome.total, eur(100));
}
