//! Extra costs (shipping, duties, fees) spread over a purchase order's lines, and the
//! landed cost of a unit: what it cost to buy and bring in, in the store's currency.
//!
//! Spreading is over the ordered quantities less what was cancelled, so a partial receipt
//! gets its share without waiting for the rest. Every division rounds half away from
//! zero at `UnitCost`'s precision, with `i128` intermediates.

use hypha_core::{Currency, ExchangeRate, Money, MoneyError, UnitCost, money::div_round};

/// How an extra cost is spread over the lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostAllocation {
    /// In proportion to each line's value (quantity × unit cost).
    #[default]
    ByValue,
    /// In proportion to each line's quantity.
    ByQuantity,
    /// Not in any unit's cost: an expense on the order's total.
    None,
}

impl CostAllocation {
    pub const ALL: [CostAllocation; 3] = [
        CostAllocation::ByValue,
        CostAllocation::ByQuantity,
        CostAllocation::None,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CostAllocation::ByValue => "by_value",
            CostAllocation::ByQuantity => "by_quantity",
            CostAllocation::None => "none",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == name)
    }
}

/// A line as spreading sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineBasis {
    /// Ordered less cancelled.
    pub quantity: u32,
    /// In the order's currency.
    pub unit_cost: UnitCost,
}

/// One extra cost to spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostShare {
    pub amount: Money,
    pub allocation: CostAllocation,
}

/// What each line carries per unit of the `costs` that are in `currency`, in that
/// currency, in the order of `lines`. A line with no quantity carries nothing. Spread by
/// value over lines whose value is all zero, a cost goes by quantity instead.
pub fn extra_per_unit(
    lines: &[LineBasis],
    costs: &[CostShare],
    currency: Currency,
) -> Result<Vec<UnitCost>, MoneyError> {
    let mut extra = vec![0_i128; lines.len()];
    let quantities: Vec<i128> = lines.iter().map(|l| i128::from(l.quantity)).collect();
    // Each fits: at most u32::MAX × 2⁵³ per line, a thousand lines.
    let values: Vec<i128> = lines
        .iter()
        .map(|l| i128::from(l.quantity) * i128::from(l.unit_cost.scaled()))
        .collect();
    let total_quantity: i128 = quantities.iter().sum();
    let total_value: i128 = values.iter().sum();
    for cost in costs.iter().filter(|c| c.amount.currency() == currency) {
        let (weights, total) = match cost.allocation {
            CostAllocation::None => continue,
            CostAllocation::ByValue if total_value > 0 => (&values, total_value),
            CostAllocation::ByValue | CostAllocation::ByQuantity => (&quantities, total_quantity),
        };
        if total == 0 {
            continue;
        }
        let amount = i128::from(cost.amount.amount()) * i128::from(UnitCost::SCALE);
        for (i, line) in lines.iter().enumerate() {
            if line.quantity == 0 {
                continue;
            }
            // The line's share, per unit: amount × weight ÷ (total × quantity). A large
            // cost on a large line can pass i128: refused, never wrapped.
            let share = amount
                .checked_mul(weights[i])
                .zip(total.checked_mul(i128::from(line.quantity)))
                .map(|(numerator, denominator)| div_round(numerator, denominator))
                .ok_or(MoneyError::Overflow)?;
            extra[i] = extra[i].checked_add(share).ok_or(MoneyError::Overflow)?;
        }
    }
    extra
        .into_iter()
        .map(|scaled| {
            let scaled = i64::try_from(scaled).map_err(|_| MoneyError::Overflow)?;
            UnitCost::new(scaled, currency)
        })
        .collect()
}

/// What one unit cost to bring in, in `store`: its unit cost and its share of the
/// order's extra costs in the order's currency, converted at `rate`, plus its share of
/// those paid in `store`. `None` when the order isn't in `store` and has no rate.
pub fn landed_unit_cost(
    unit_cost: UnitCost,
    extra_in_order_currency: UnitCost,
    extra_in_store_currency: UnitCost,
    rate: Option<ExchangeRate>,
    store: Currency,
) -> Result<Option<UnitCost>, MoneyError> {
    let base = unit_cost.checked_add(extra_in_order_currency)?;
    let converted = if base.currency() == store {
        base
    } else {
        let Some(rate) = rate else { return Ok(None) };
        rate.convert(base, store)?
    };
    converted.checked_add(extra_in_store_currency).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(minor: &str) -> UnitCost {
        UnitCost::parse(minor, Currency::EUR).unwrap()
    }

    fn line(quantity: u32, minor: &str) -> LineBasis {
        LineBasis {
            quantity,
            unit_cost: eur(minor),
        }
    }

    fn cost(amount: i64, allocation: CostAllocation) -> CostShare {
        CostShare {
            amount: Money::new(amount, Currency::EUR).unwrap(),
            allocation,
        }
    }

    fn spread(lines: &[LineBasis], costs: &[CostShare]) -> Vec<String> {
        extra_per_unit(lines, costs, Currency::EUR)
            .unwrap()
            .iter()
            .map(UnitCost::minor_units)
            .collect()
    }

    #[test]
    fn a_spread_past_i128_is_an_overflow_not_a_wrap() {
        let most = UnitCost::new(UnitCost::MAX_SCALED, Currency::EUR).unwrap();
        let lines = [
            LineBasis {
                quantity: 1_000_000_000,
                unit_cost: most,
            },
            line(1, "100"),
        ];
        let huge = CostShare {
            amount: Money::new(Money::MAX_AMOUNT, Currency::EUR).unwrap(),
            allocation: CostAllocation::ByValue,
        };
        assert_eq!(
            extra_per_unit(&lines, &[huge], Currency::EUR),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn by_value_follows_each_lines_share_of_the_value() {
        // Values 10 × 100 = 1000 and 10 × 300 = 3000: €10.00 shipping splits 250 / 750
        // cents, so 25 and 75 cents a unit.
        let lines = [line(10, "100"), line(10, "300")];
        assert_eq!(
            spread(&lines, &[cost(1000, CostAllocation::ByValue)]),
            ["25", "75"]
        );
    }

    #[test]
    fn by_quantity_follows_each_lines_share_of_the_units() {
        // 1 and 3 units: €10.00 splits 250 / 750 cents, 250 cents a unit for both.
        let lines = [line(1, "100"), line(3, "300")];
        assert_eq!(
            spread(&lines, &[cost(1000, CostAllocation::ByQuantity)]),
            ["250", "250"]
        );
    }

    #[test]
    fn a_share_rounds_half_away_from_zero_at_four_places() {
        // 1000 cents over 3 units: 333.33333… → 333.3333
        assert_eq!(
            spread(&[line(3, "100")], &[cost(1000, CostAllocation::ByQuantity)]),
            ["333.3333"]
        );
        // 2000 cents over 3: 666.66666… → 666.6667
        assert_eq!(
            spread(&[line(3, "100")], &[cost(2000, CostAllocation::ByQuantity)]),
            ["666.6667"]
        );
        // 1 cent over 20,000 units: 0.00005 → 0.0001 (a half rounds up)
        assert_eq!(
            spread(&[line(20_000, "1")], &[cost(1, CostAllocation::ByQuantity)]),
            ["0.0001"]
        );
        // 1 cent over 20,001 units: just under half → 0
        assert_eq!(
            spread(&[line(20_001, "1")], &[cost(1, CostAllocation::ByQuantity)]),
            ["0"]
        );
    }

    #[test]
    fn costs_add_up_per_line() {
        let lines = [line(2, "100"), line(2, "100")];
        let costs = [
            cost(400, CostAllocation::ByValue),
            cost(200, CostAllocation::ByQuantity),
        ];
        // 100 + 50 cents a unit each.
        assert_eq!(spread(&lines, &costs), ["150", "150"]);
    }

    #[test]
    fn an_expense_reaches_no_unit() {
        assert_eq!(
            spread(&[line(2, "100")], &[cost(1000, CostAllocation::None)]),
            ["0"]
        );
    }

    #[test]
    fn a_line_with_nothing_left_to_order_carries_nothing() {
        // The cancelled line is left out: the other carries the whole cost.
        let lines = [line(0, "100"), line(4, "100")];
        assert_eq!(
            spread(&lines, &[cost(1000, CostAllocation::ByValue)]),
            ["0", "250"]
        );
    }

    #[test]
    fn free_goods_are_spread_by_quantity() {
        let lines = [line(1, "0"), line(3, "0")];
        assert_eq!(
            spread(&lines, &[cost(400, CostAllocation::ByValue)]),
            ["100", "100"]
        );
    }

    #[test]
    fn costs_in_another_currency_are_left_for_their_own_spread() {
        let usd = CostShare {
            amount: Money::new(1000, Currency::USD).unwrap(),
            allocation: CostAllocation::ByValue,
        };
        assert_eq!(spread(&[line(2, "100")], &[usd]), ["0"]);
        assert_eq!(
            extra_per_unit(&[line(2, "100")], &[usd], Currency::USD)
                .unwrap()
                .iter()
                .map(UnitCost::minor_units)
                .collect::<Vec<_>>(),
            ["500"]
        );
    }

    #[test]
    fn no_lines_carry_nothing() {
        assert_eq!(
            spread(&[], &[cost(1000, CostAllocation::ByValue)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn landed_cost_in_the_stores_currency_adds_the_shares() {
        assert_eq!(
            landed_unit_cost(eur("100"), eur("25"), eur("10"), None, Currency::EUR),
            Ok(Some(eur("135")))
        );
    }

    #[test]
    fn landed_cost_converts_the_orders_part_at_the_rate() {
        let usd = |minor: &str| UnitCost::parse(minor, Currency::USD).unwrap();
        let rate = ExchangeRate::parse("0.9").unwrap();
        // ($10.00 + $1.00) × 0.9 = €9.90, plus €0.50 of duties paid in euros.
        assert_eq!(
            landed_unit_cost(
                usd("1000"),
                usd("100"),
                eur("50"),
                Some(rate),
                Currency::EUR
            ),
            Ok(Some(eur("1040")))
        );
        // At 1/3, $1.00 is €0.333333… → 33.3333 cents.
        let third = ExchangeRate::parse("0.33333333").unwrap();
        assert_eq!(
            landed_unit_cost(usd("100"), usd("0"), eur("0"), Some(third), Currency::EUR),
            Ok(Some(eur("33.3333")))
        );
    }

    #[test]
    fn landed_cost_without_a_rate_is_unknown() {
        let usd = UnitCost::parse("100", Currency::USD).unwrap();
        let none = UnitCost::zero(Currency::USD);
        assert_eq!(
            landed_unit_cost(usd, none, eur("0"), None, Currency::EUR),
            Ok(None)
        );
    }

    #[test]
    fn allocations_read_back_by_name() {
        for allocation in CostAllocation::ALL {
            assert_eq!(CostAllocation::parse(allocation.as_str()), Some(allocation));
        }
        assert_eq!(CostAllocation::parse("by_weight"), None);
    }
}
