//! A discount in one sentence, built from its data: what the admin, emails, the storefront
//! and agents all show, so they say the same.
//!
//! The discount names products, categories and groups only by ID, so the sentence counts
//! them ("2 categories"). Amounts are shown in `market` when it has one, else every
//! market's, and dates in UTC.

use hypha_core::{BasisPoints, MarketId, Money, Timestamp};

use crate::discount::{
    Allocation, Condition, Discount, ItemSet, Items, Kind, Measure, PerMarket, Target, Threshold,
    Value,
};

/// "10% off 2 categories · orders from 50.00 EUR · until 31 Oct 2026".
pub fn summary(discount: &Discount, market: Option<MarketId>) -> String {
    let mut parts = vec![kind_phrase(discount.kind(), market)];
    for condition in discount.conditions() {
        parts.push(condition_phrase(condition, market));
    }
    let limits = discount.limits();
    if let Some(total) = limits.total {
        parts.push(plural(u64::from(total), "use", "uses"));
    }
    if limits.once_per_customer {
        parts.push("once per customer".to_string());
    }
    if let Some(start) = discount.starts_at() {
        parts.push(format!("from {}", date(start)));
    }
    if let Some(end) = discount.ends_at() {
        // The end is exclusive: a discount ending at midnight runs until the day before.
        parts.push(format!(
            "until {}",
            date(Timestamp::from_unix_micros(
                end.unix_micros().saturating_sub(1)
            ))
        ));
    }
    parts.join(" · ")
}

fn kind_phrase(kind: &Kind, market: Option<MarketId>) -> String {
    match kind {
        Kind::Standard { rewards } => rewards
            .iter()
            .map(|r| reward_phrase(&r.target, &r.value, market))
            .collect::<Vec<_>>()
            .join(" + "),
        Kind::BuyXGetY {
            buys,
            buy,
            gets,
            get_quantity,
            value,
            uses_per_order,
        } => {
            let buy = match buy {
                Threshold::Quantity(n) => format!("{n} × {}", items_phrase(buys)),
                Threshold::Amount(amounts) => {
                    format!(
                        "{} of {}",
                        amounts_phrase(amounts, market),
                        items_phrase(buys)
                    )
                }
            };
            let got = match value {
                Value::Percent(bp) if bp.value() == 10_000 => "free".to_string(),
                Value::Percent(bp) => format!("at {} off", percent(*bp)),
                Value::Amount { amounts, .. } | Value::FixedPrice(amounts) => {
                    format!("at {} off each", amounts_phrase(amounts, market))
                }
            };
            let mut phrase = format!(
                "Buy {buy}, get {get_quantity} × {} {got}",
                items_phrase(gets)
            );
            if let Some(times) = uses_per_order {
                phrase.push_str(&format!(
                    " (up to {} per order)",
                    plural(u64::from(*times), "time", "times")
                ));
            }
            phrase
        }
        Kind::Tiered {
            items,
            measure,
            tiers,
        } => {
            let steps: Vec<String> = tiers
                .iter()
                .map(|t| {
                    let from = match (&t.threshold, measure) {
                        (Threshold::Quantity(n), _) => plural(u64::from(*n), "item", "items"),
                        (Threshold::Amount(amounts), _) => amounts_phrase(amounts, market),
                    };
                    format!("{} from {from}", value_off(&t.value, market))
                })
                .collect();
            let measured = match measure {
                Measure::Quantity => "",
                Measure::Subtotal => " spent",
            };
            format!("{} on {}{measured}", steps.join(", "), items_phrase(items))
        }
    }
}

fn reward_phrase(target: &Target, value: &Value, market: Option<MarketId>) -> String {
    match target {
        Target::Items(set) => match value {
            Value::Percent(bp) => format!("{} off {}", percent(*bp), items_phrase(set)),
            Value::Amount {
                amounts,
                allocation,
            } => {
                let amount = amounts_phrase(amounts, market);
                match allocation {
                    Allocation::Each => format!("{amount} off each of {}", items_phrase(set)),
                    Allocation::Across => format!("{amount} off {}", items_phrase(set)),
                    Allocation::Cheapest { units } => format!(
                        "{amount} off the cheapest {} of {}",
                        plural(u64::from(*units), "item", "items"),
                        items_phrase(set)
                    ),
                }
            }
            Value::FixedPrice(amounts) => format!(
                "{} at {} each",
                capitalized(&items_phrase(set)),
                amounts_phrase(amounts, market)
            ),
        },
        Target::Subtotal => format!("{} the order", value_off(value, market)),
        Target::Shipping { max_amount } => {
            let mut phrase = match value {
                Value::Percent(bp) if bp.value() == 10_000 => "Free shipping".to_string(),
                _ => format!("{} shipping", value_off(value, market)),
            };
            if let Some(max) = max_amount {
                phrase.push_str(&format!(" up to {}", amounts_phrase(max, market)));
            }
            phrase
        }
    }
}

/// "10% off", "5.00 EUR off".
fn value_off(value: &Value, market: Option<MarketId>) -> String {
    match value {
        Value::Percent(bp) => format!("{} off", percent(*bp)),
        Value::Amount { amounts, .. } | Value::FixedPrice(amounts) => {
            format!("{} off", amounts_phrase(amounts, market))
        }
    }
}

fn condition_phrase(condition: &Condition, market: Option<MarketId>) -> String {
    match condition {
        Condition::Markets(ids) => format!("in {}", plural(ids.len() as u64, "market", "markets")),
        Condition::Channels(ids) => {
            format!("in {}", plural(ids.len() as u64, "channel", "channels"))
        }
        Condition::CustomerGroups(ids) => format!(
            "for {}",
            plural(ids.len() as u64, "customer group", "customer groups")
        ),
        Condition::Customers(ids) => {
            format!("for {}", plural(ids.len() as u64, "customer", "customers"))
        }
        Condition::SignedIn => "signed-in customers".to_string(),
        Condition::MinSubtotal(amounts) => {
            format!("orders from {}", amounts_phrase(amounts, market))
        }
        Condition::MinQuantity(n) => format!("from {}", plural(u64::from(*n), "item", "items")),
    }
}

fn items_phrase(set: &ItemSet) -> String {
    let mut phrase = match &set.items {
        Items::All => "all items".to_string(),
        Items::Products(ids) => plural(ids.len() as u64, "product", "products"),
        Items::Variants(ids) => plural(ids.len() as u64, "variant", "variants"),
        Items::Categories(ids) => plural(ids.len() as u64, "category", "categories"),
    };
    if set.exclude_on_sale {
        phrase.push_str(" not on sale");
    }
    phrase
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// 1000 → "10%", 1250 → "12.5%", 1234 → "12.34%".
pub(crate) fn percent(bp: BasisPoints) -> String {
    let v = bp.value();
    let (whole, frac) = (v / 100, (v % 100).abs());
    match frac {
        0 => format!("{whole}%"),
        f if f % 10 == 0 => format!("{whole}.{}%", f / 10),
        f => format!("{whole}.{f:02}%"),
    }
}

/// The market's amount when it has one, else every market's: "5.00 EUR / 600 JPY".
fn amounts_phrase(amounts: &PerMarket, market: Option<MarketId>) -> String {
    if let Some(amount) = market.and_then(|m| amounts.get(m)) {
        return money(amount);
    }
    amounts
        .iter()
        .map(|(_, a)| money(a))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// "5.00 EUR", in the currency's minor units.
pub fn money(m: Money) -> String {
    let units = u32::from(m.currency().minor_units());
    let sign = if m.amount() < 0 { "-" } else { "" };
    let abs = m.amount().unsigned_abs();
    if units == 0 {
        return format!("{sign}{abs} {}", m.currency());
    }
    let scale = 10u64.pow(units);
    format!(
        "{sign}{}.{:0width$} {}",
        abs / scale,
        abs % scale,
        m.currency(),
        width = units as usize
    )
}

/// "31 Oct 2026", the UTC day of `at`.
fn date(at: Timestamp) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = at.unix_micros().div_euclid(86_400_000_000);
    let (y, m, d) = civil_from_days(days);
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use hypha_core::{CategoryId, Currency, DiscountId};
    use uuid::Uuid;

    use super::*;
    use crate::discount::{ClassSet, DiscountDraft, Limits, Method, Reward};

    fn mkt(n: u128) -> MarketId {
        MarketId::from_uuid(Uuid::from_u128(n))
    }

    fn eur(n: i64) -> Money {
        Money::new(n, Currency::EUR).unwrap()
    }

    fn bp(n: i32) -> BasisPoints {
        BasisPoints::new(n).unwrap()
    }

    fn discount(kind: Kind) -> DiscountDraft {
        DiscountDraft::new("Offer", Method::Automatic, kind)
    }

    fn of(draft: DiscountDraft) -> String {
        summary(
            &Discount::new(DiscountId::from_uuid(Uuid::from_u128(1)), draft).unwrap(),
            Some(mkt(1)),
        )
    }

    fn standard(target: Target, value: Value) -> Kind {
        Kind::Standard {
            rewards: vec![Reward { target, value }],
        }
    }

    #[test]
    fn a_percentage_off_categories_with_a_minimum_and_an_end() {
        let mut draft = discount(standard(
            Target::Items(ItemSet {
                items: Items::Categories(vec![
                    CategoryId::from_uuid(Uuid::from_u128(1)),
                    CategoryId::from_uuid(Uuid::from_u128(2)),
                ]),
                exclude_on_sale: false,
            }),
            Value::Percent(bp(1000)),
        ));
        draft.conditions = vec![Condition::MinSubtotal(PerMarket::of(mkt(1), eur(5000)))];
        // Midnight on 1 Nov: the last day is 31 Oct.
        draft.ends_at = Some(Timestamp::from_unix_micros(1_793_491_200_000_000));
        assert_eq!(
            of(draft),
            "10% off 2 categories · orders from 50.00 EUR · until 31 Oct 2026"
        );
    }

    #[test]
    fn free_shipping_says_free() {
        let draft = discount(standard(
            Target::Shipping { max_amount: None },
            Value::Percent(bp(10_000)),
        ));
        assert_eq!(of(draft), "Free shipping");
    }

    #[test]
    fn an_amount_off_the_order_shows_the_markets_amount() {
        let draft = discount(standard(
            Target::Subtotal,
            Value::Amount {
                amounts: PerMarket::new([(mkt(1), eur(1500)), (mkt(2), eur(1600))].into()),
                allocation: Allocation::Across,
            },
        ));
        assert_eq!(of(draft), "15.00 EUR off the order");
    }

    #[test]
    fn without_a_market_every_markets_amount_is_shown() {
        let draft = discount(standard(
            Target::Subtotal,
            Value::Amount {
                amounts: PerMarket::new(
                    [
                        (mkt(1), eur(1500)),
                        (
                            mkt(2),
                            Money::new(1800, Currency::new("JPY").unwrap()).unwrap(),
                        ),
                    ]
                    .into(),
                ),
                allocation: Allocation::Across,
            },
        ));
        let d = Discount::new(DiscountId::from_uuid(Uuid::from_u128(1)), draft).unwrap();
        assert_eq!(summary(&d, None), "15.00 EUR / 1800 JPY off the order");
    }

    #[test]
    fn limits_and_a_start_are_listed() {
        let mut draft = discount(standard(Target::Subtotal, Value::Percent(bp(1250))));
        draft.limits = Limits {
            total: Some(1),
            once_per_customer: true,
        };
        draft.starts_at = Some(Timestamp::UNIX_EPOCH);
        draft.combines_with = ClassSet::ALL;
        assert_eq!(
            of(draft),
            "12.5% off the order · 1 use · once per customer · from 1 Jan 1970"
        );
    }

    #[test]
    fn buy_x_get_y_reads_as_a_deal() {
        let draft = discount(Kind::BuyXGetY {
            buys: ItemSet::all(),
            buy: Threshold::Quantity(2),
            gets: ItemSet::all(),
            get_quantity: 1,
            value: Value::Percent(bp(10_000)),
            uses_per_order: Some(1),
        });
        assert_eq!(
            of(draft),
            "Buy 2 × all items, get 1 × all items free (up to 1 time per order)"
        );
    }

    #[test]
    fn tiers_list_each_step() {
        let draft = discount(Kind::Tiered {
            items: ItemSet::all(),
            measure: Measure::Quantity,
            tiers: vec![
                crate::Tier {
                    threshold: Threshold::Quantity(3),
                    value: Value::Percent(bp(1000)),
                },
                crate::Tier {
                    threshold: Threshold::Quantity(5),
                    value: Value::Percent(bp(1500)),
                },
            ],
        });
        assert_eq!(
            of(draft),
            "10% off from 3 items, 15% off from 5 items on all items"
        );
    }

    #[test]
    fn percentages_keep_only_the_digits_they_need() {
        assert_eq!(percent(bp(1000)), "10%");
        assert_eq!(percent(bp(1250)), "12.5%");
        assert_eq!(percent(bp(1234)), "12.34%");
        assert_eq!(percent(bp(5)), "0.05%");
    }

    #[test]
    fn money_uses_the_currencys_minor_units() {
        assert_eq!(money(eur(5)), "0.05 EUR");
        assert_eq!(
            money(Money::new(600, Currency::new("JPY").unwrap()).unwrap()),
            "600 JPY"
        );
    }

    #[test]
    fn dates_are_utc_days() {
        assert_eq!(
            date(Timestamp::from_unix_micros(951_782_400_000_000)),
            "29 Feb 2000"
        );
        assert_eq!(date(Timestamp::from_unix_micros(-1)), "31 Dec 1969");
    }
}
