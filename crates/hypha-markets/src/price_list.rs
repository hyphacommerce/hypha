use std::collections::HashMap;

use hypha_core::{BasisPoints, Currency, Price, PriceListId, VariantId};

use crate::error::{MismatchedPrice, PriceListError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceList {
    id: PriceListId,
    currency: Currency,
    adjustment: BasisPoints,
    fixed_prices: HashMap<VariantId, Price>,
}

impl PriceList {
    pub fn new(
        id: PriceListId,
        currency: Currency,
        adjustment: BasisPoints,
        fixed_prices: HashMap<VariantId, Price>,
    ) -> Result<Self, PriceListError> {
        // Collect every mismatch so callers (and agents) can fix them all in one go.
        // HashMap order is random, so sort to keep the error deterministic.
        let mut mismatches: Vec<MismatchedPrice> = fixed_prices
            .iter()
            .filter(|(_, price)| price.money().currency() != currency)
            .map(|(variant, price)| MismatchedPrice {
                variant: *variant,
                found: price.money().currency(),
            })
            .collect();

        if !mismatches.is_empty() {
            mismatches.sort_by_key(|m| m.variant.as_uuid());
            return Err(PriceListError::CurrencyMismatch {
                expected: currency,
                prices: mismatches,
            });
        }

        Ok(Self {
            id,
            currency,
            adjustment,
            fixed_prices,
        })
    }

    pub fn set_fixed_price(
        &mut self,
        variant: VariantId,
        price: Price,
    ) -> Result<(), PriceListError> {
        let found = price.money().currency();
        if found != self.currency {
            return Err(PriceListError::CurrencyMismatch {
                expected: self.currency,
                prices: vec![MismatchedPrice { variant, found }],
            });
        }
        self.fixed_prices.insert(variant, price);
        Ok(())
    }

    /// Returns the removed price, or `None` if the variant had no fixed price.
    pub fn remove_fixed_price(&mut self, variant: VariantId) -> Option<Price> {
        self.fixed_prices.remove(&variant)
    }

    pub fn set_adjustment(&mut self, adjustment: BasisPoints) {
        self.adjustment = adjustment;
    }

    pub fn id(&self) -> PriceListId {
        self.id
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn adjustment(&self) -> BasisPoints {
        self.adjustment
    }

    pub fn fixed_price(&self, variant: VariantId) -> Option<Price> {
        self.fixed_prices.get(&variant).copied()
    }

    pub fn fixed_prices(&self) -> &HashMap<VariantId, Price> {
        &self.fixed_prices
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::Money;
    use uuid::Uuid;

    use super::*;

    fn plid() -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(1))
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn price(amount: i64, currency: Currency) -> Price {
        Price::new(Money::new(amount, currency)).unwrap()
    }

    fn eur_list() -> PriceList {
        PriceList::new(plid(), Currency::Eur, BasisPoints::ZERO, HashMap::new()).unwrap()
    }

    #[test]
    fn new_accepts_prices_in_list_currency() {
        let prices = HashMap::from([(vid(1), price(1000, Currency::Eur))]);
        let list = PriceList::new(plid(), Currency::Eur, BasisPoints::ZERO, prices).unwrap();
        assert_eq!(list.fixed_price(vid(1)), Some(price(1000, Currency::Eur)));
    }

    #[test]
    fn new_rejects_price_in_other_currency() {
        let prices = HashMap::from([(vid(1), price(1000, Currency::Usd))]);
        assert_eq!(
            PriceList::new(plid(), Currency::Eur, BasisPoints::ZERO, prices),
            Err(PriceListError::CurrencyMismatch {
                expected: Currency::Eur,
                prices: vec![MismatchedPrice {
                    variant: vid(1),
                    found: Currency::Usd,
                }],
            })
        );
    }

    #[test]
    fn new_reports_every_mismatch_sorted_by_variant() {
        let prices = HashMap::from([
            (vid(3), price(300, Currency::Rsd)),
            (vid(2), price(200, Currency::Eur)),
            (vid(1), price(100, Currency::Usd)),
        ]);
        assert_eq!(
            PriceList::new(plid(), Currency::Eur, BasisPoints::ZERO, prices),
            Err(PriceListError::CurrencyMismatch {
                expected: Currency::Eur,
                prices: vec![
                    MismatchedPrice {
                        variant: vid(1),
                        found: Currency::Usd,
                    },
                    MismatchedPrice {
                        variant: vid(3),
                        found: Currency::Rsd,
                    },
                ],
            })
        );
    }

    #[test]
    fn new_keeps_adjustment() {
        let markup = BasisPoints::new(1500).unwrap();
        let list = PriceList::new(plid(), Currency::Eur, markup, HashMap::new()).unwrap();
        assert_eq!(list.adjustment(), markup);
    }

    #[test]
    fn set_fixed_price_overwrites_existing() {
        let mut list = eur_list();
        list.set_fixed_price(vid(1), price(1000, Currency::Eur))
            .unwrap();
        assert_eq!(
            list.set_fixed_price(vid(1), price(900, Currency::Eur)),
            Ok(())
        );
        assert_eq!(list.fixed_price(vid(1)), Some(price(900, Currency::Eur)));
    }

    #[test]
    fn set_fixed_price_rejects_other_currency_and_keeps_old_price() {
        let mut list = eur_list();
        list.set_fixed_price(vid(1), price(1000, Currency::Eur))
            .unwrap();
        assert_eq!(
            list.set_fixed_price(vid(1), price(1000, Currency::Rsd)),
            Err(PriceListError::CurrencyMismatch {
                expected: Currency::Eur,
                prices: vec![MismatchedPrice {
                    variant: vid(1),
                    found: Currency::Rsd,
                }],
            })
        );
        assert_eq!(list.fixed_price(vid(1)), Some(price(1000, Currency::Eur)));
    }

    #[test]
    fn remove_fixed_price_returns_removed_price() {
        let mut list = eur_list();
        list.set_fixed_price(vid(1), price(1000, Currency::Eur))
            .unwrap();
        assert_eq!(
            list.remove_fixed_price(vid(1)),
            Some(price(1000, Currency::Eur))
        );
        assert_eq!(list.fixed_price(vid(1)), None);
    }

    #[test]
    fn remove_missing_fixed_price_returns_none() {
        assert_eq!(eur_list().remove_fixed_price(vid(1)), None);
    }

    #[test]
    fn set_adjustment_replaces_it() {
        let mut list = eur_list();
        let discount = BasisPoints::new(-2000).unwrap();
        list.set_adjustment(discount);
        assert_eq!(list.adjustment(), discount);
    }
}
