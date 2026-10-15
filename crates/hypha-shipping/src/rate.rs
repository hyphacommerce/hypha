//! Built-in rates (design note 0015, SH4): computed by Hypha, no I/O.
//!
//! One shape covers the four kinds the admin offers: a table of tiers per currency over a
//! basis (the group's subtotal or its weight). Flat is one open tier; free over a
//! subtotal is two subtotal tiers, the second free; tiers are tiers.

use hypha_core::{Currency, Money, Price, TotalWeight};

use crate::ShippingError;

/// What a rate's tiers are bounded by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateBasis {
    /// One price per currency, whatever the group: each table has one open tier.
    Flat,
    /// The group's subtotal, in minor units of the table's currency.
    Subtotal,
    /// The group's weight, in grams.
    Weight,
}

impl RateBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            RateBasis::Flat => "flat",
            RateBasis::Subtotal => "subtotal",
            RateBasis::Weight => "weight",
        }
    }
}

/// One tier: its price applies while the basis is below `up_to` (exclusive). The last
/// tier of a table is open (`None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tier {
    pub up_to: Option<u64>,
    pub price: Price,
}

/// The tiers in one currency, ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateTable {
    currency: Currency,
    tiers: Vec<Tier>,
}

impl RateTable {
    /// Tiers ascend strictly, the last is open and only the last, and every price is in
    /// the table's currency.
    pub fn new(currency: Currency, tiers: Vec<Tier>) -> Result<Self, ShippingError> {
        let Some((last, bounded)) = tiers.split_last() else {
            return Err(ShippingError::EmptyRateTable(currency));
        };
        if last.up_to.is_some() {
            return Err(ShippingError::LastTierBounded(currency));
        }
        let mut below = 0u64;
        for tier in bounded {
            match tier.up_to {
                Some(up_to) if up_to > below => below = up_to,
                _ => return Err(ShippingError::TiersNotAscending(currency)),
            }
        }
        if let Some(other) = tiers
            .iter()
            .map(|t| t.price.money().currency())
            .find(|c| *c != currency)
        {
            return Err(ShippingError::TierCurrencyMismatch {
                table: currency,
                tier: other,
            });
        }
        Ok(Self { currency, tiers })
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn tiers(&self) -> &[Tier] {
        &self.tiers
    }

    /// The price of the first tier `value` is below, else the open tier. `None` only for
    /// an empty table, which `new` refuses.
    fn price_for(&self, value: u64) -> Option<Price> {
        self.tiers
            .iter()
            .find(|t| t.up_to.is_none_or(|up_to| value < up_to))
            .map(|t| t.price)
    }
}

/// A built-in rate: a basis and a table per currency it is offered in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinRate {
    basis: RateBasis,
    tables: Vec<RateTable>,
}

/// Why a built-in rate has no price for a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unpriced {
    /// No table in the cart's currency: the option isn't offered there.
    NoPriceInCurrency,
    /// Weight tiers, and a line's weight isn't known.
    WeightUnknown,
}

impl BuiltinRate {
    /// At least one table, one per currency.
    pub fn new(basis: RateBasis, tables: Vec<RateTable>) -> Result<Self, ShippingError> {
        if tables.is_empty() {
            return Err(ShippingError::NoRateTables);
        }
        for (i, table) in tables.iter().enumerate() {
            if tables[..i].iter().any(|t| t.currency == table.currency) {
                return Err(ShippingError::DuplicateRateCurrency(table.currency));
            }
            if basis == RateBasis::Flat && table.tiers.len() != 1 {
                return Err(ShippingError::FlatRateWithTiers(table.currency));
            }
        }
        Ok(Self { basis, tables })
    }

    /// One price in each currency, whatever the cart.
    pub fn flat(prices: Vec<Price>) -> Result<Self, ShippingError> {
        let tables = prices
            .into_iter()
            .map(|price| {
                RateTable::new(price.money().currency(), vec![Tier { up_to: None, price }])
            })
            .collect::<Result<_, _>>()?;
        Self::new(RateBasis::Flat, tables)
    }

    pub fn basis(&self) -> RateBasis {
        self.basis
    }

    pub fn tables(&self) -> &[RateTable] {
        &self.tables
    }

    /// The price for a group with this subtotal and (when every line's is known) weight.
    pub fn price(&self, subtotal: Money, weight: Option<TotalWeight>) -> Result<Price, Unpriced> {
        let table = self
            .tables
            .iter()
            .find(|t| t.currency == subtotal.currency())
            .ok_or(Unpriced::NoPriceInCurrency)?;
        let value = match self.basis {
            RateBasis::Flat => 0,
            // A subtotal is never negative (prices aren't); clamped all the same.
            RateBasis::Subtotal => u64::try_from(subtotal.amount()).unwrap_or(0),
            RateBasis::Weight => weight.ok_or(Unpriced::WeightUnknown)?.grams(),
        };
        table.price_for(value).ok_or(Unpriced::NoPriceInCurrency)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, Money, Price, TotalWeight, Weight};

    use super::*;

    fn eur(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
    }
    fn usd(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Usd).unwrap()).unwrap()
    }
    fn tier(up_to: Option<u64>, price: Price) -> Tier {
        Tier { up_to, price }
    }
    fn subtotal(amount: i64) -> Money {
        Money::new(amount, Currency::Eur).unwrap()
    }
    fn grams(g: u32) -> Option<TotalWeight> {
        Some(TotalWeight::default().add(Weight::from_grams(g), 1))
    }

    /// Free over 50.00 EUR, 4.90 below.
    fn free_over_50() -> BuiltinRate {
        let table = RateTable::new(
            Currency::Eur,
            vec![tier(Some(5000), eur(490)), tier(None, eur(0))],
        )
        .unwrap();
        BuiltinRate::new(RateBasis::Subtotal, vec![table]).unwrap()
    }

    #[test]
    fn a_flat_rate_is_one_price_per_currency() {
        let rate = BuiltinRate::flat(vec![eur(500), usd(600)]).unwrap();
        assert_eq!(rate.price(subtotal(1), None), Ok(eur(500)));
        assert_eq!(
            rate.price(Money::new(99_999, Currency::Usd).unwrap(), None),
            Ok(usd(600))
        );
    }

    #[test]
    fn a_tier_applies_below_its_bound_exclusive() {
        let rate = free_over_50();
        assert_eq!(rate.price(subtotal(4999), None), Ok(eur(490)));
        assert_eq!(rate.price(subtotal(5000), None), Ok(eur(0)));
        assert_eq!(rate.price(subtotal(0), None), Ok(eur(490)));
    }

    #[test]
    fn weight_tiers_need_the_weight() {
        let table = RateTable::new(
            Currency::Eur,
            vec![
                tier(Some(1000), eur(400)),
                tier(Some(5000), eur(800)),
                tier(None, eur(1500)),
            ],
        )
        .unwrap();
        let rate = BuiltinRate::new(RateBasis::Weight, vec![table]).unwrap();
        assert_eq!(rate.price(subtotal(100), grams(999)), Ok(eur(400)));
        assert_eq!(rate.price(subtotal(100), grams(1000)), Ok(eur(800)));
        assert_eq!(rate.price(subtotal(100), grams(20_000)), Ok(eur(1500)));
        assert_eq!(
            rate.price(subtotal(100), None),
            Err(Unpriced::WeightUnknown)
        );
    }

    #[test]
    fn no_table_in_the_currency_is_no_price() {
        let rate = BuiltinRate::flat(vec![usd(600)]).unwrap();
        assert_eq!(
            rate.price(subtotal(1), None),
            Err(Unpriced::NoPriceInCurrency)
        );
    }

    #[test]
    fn a_table_needs_tiers_rising_to_an_open_last_one() {
        assert_eq!(
            RateTable::new(Currency::Eur, vec![]),
            Err(ShippingError::EmptyRateTable(Currency::Eur))
        );
        assert_eq!(
            RateTable::new(Currency::Eur, vec![tier(Some(10), eur(1))]),
            Err(ShippingError::LastTierBounded(Currency::Eur))
        );
        assert_eq!(
            RateTable::new(
                Currency::Eur,
                vec![
                    tier(Some(10), eur(1)),
                    tier(Some(10), eur(2)),
                    tier(None, eur(3))
                ]
            ),
            Err(ShippingError::TiersNotAscending(Currency::Eur))
        );
        assert_eq!(
            RateTable::new(
                Currency::Eur,
                vec![tier(Some(0), eur(1)), tier(None, eur(3))]
            ),
            Err(ShippingError::TiersNotAscending(Currency::Eur))
        );
        assert_eq!(
            RateTable::new(Currency::Eur, vec![tier(None, usd(1))]),
            Err(ShippingError::TierCurrencyMismatch {
                table: Currency::Eur,
                tier: Currency::Usd
            })
        );
    }

    #[test]
    fn a_rate_has_one_table_per_currency_and_flat_ones_have_one_tier() {
        assert_eq!(BuiltinRate::flat(vec![]), Err(ShippingError::NoRateTables));
        assert_eq!(
            BuiltinRate::flat(vec![eur(1), eur(2)]),
            Err(ShippingError::DuplicateRateCurrency(Currency::Eur))
        );
        let tiered = RateTable::new(
            Currency::Eur,
            vec![tier(Some(10), eur(1)), tier(None, eur(2))],
        )
        .unwrap();
        assert_eq!(
            BuiltinRate::new(RateBasis::Flat, vec![tiered]),
            Err(ShippingError::FlatRateWithTiers(Currency::Eur))
        );
    }
}
