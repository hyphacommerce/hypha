use std::collections::HashSet;

use hypha_core::{
    BasisPoints, Currency, PendingEvent, Price, PriceListId, Recorded, Recorder, Subject,
    VariantId, Version, Versioned,
};

use crate::{
    error::{MismatchedPrice, PriceListError},
    events::{PRICE_LIST, PriceListChanged},
};

/// A price list: a currency, a percentage adjustment, and fixed per-variant prices.
///
/// The fixed prices are not held here. A list can have one per variant in a large
/// catalog, so they live behind `PriceListRepo::{set_fixed_prices, fixed_prices}`
/// and are read in batches. The rules for them stay here: see `check_fixed_prices`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceList {
    id: PriceListId,
    version: Version,
    currency: Currency,
    adjustment: BasisPoints,
    events: Recorder,
}

impl Recorded for PriceList {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for PriceList {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl PriceList {
    /// The adjustment is a `BasisPoints`, so it is valid by construction.
    pub fn new(id: PriceListId, currency: Currency, adjustment: BasisPoints) -> Self {
        let mut list = Self {
            id,
            version: Version::NEW,
            currency,
            adjustment,
            events: Recorder::default(),
        };
        list.changed();
        list
    }

    fn changed(&mut self) {
        let event = PriceListChanged {
            price_list: self.id,
        };
        self.events
            .record_once(Subject::new(PRICE_LIST, self.id.as_uuid()), &event);
    }

    /// Validates fixed prices before they are stored for this list:
    /// each variant at most once, and every price in the list's currency.
    ///
    /// Reports every currency mismatch at once, sorted by variant ID, so a caller
    /// can fix them all in one go.
    pub fn check_fixed_prices(&self, prices: &[(VariantId, Price)]) -> Result<(), PriceListError> {
        let mut seen = HashSet::with_capacity(prices.len());
        for (variant, _) in prices {
            if !seen.insert(*variant) {
                return Err(PriceListError::DuplicateFixedPrice(*variant));
            }
        }

        let mut mismatches: Vec<MismatchedPrice> = prices
            .iter()
            .filter(|(_, price)| price.money().currency() != self.currency)
            .map(|(variant, price)| MismatchedPrice {
                variant: *variant,
                found: price.money().currency(),
            })
            .collect();

        if mismatches.is_empty() {
            return Ok(());
        }
        mismatches.sort_by_key(|m| m.variant.as_uuid());
        Err(PriceListError::CurrencyMismatch {
            expected: self.currency,
            prices: mismatches,
        })
    }

    pub fn set_adjustment(&mut self, adjustment: BasisPoints) {
        if self.adjustment != adjustment {
            self.adjustment = adjustment;
            self.changed();
        }
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
}

/// Plain-data form of a `PriceList` for storage adapters (design note 0001, 3.4).
/// Its fixed prices are stored separately (see `PriceListRepo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceListSnapshot {
    pub id: PriceListId,
    pub version: Version,
    pub currency: Currency,
    pub adjustment: BasisPoints,
}

impl PriceList {
    pub fn snapshot(&self) -> PriceListSnapshot {
        PriceListSnapshot {
            id: self.id,
            version: self.version,
            currency: self.currency,
            adjustment: self.adjustment,
        }
    }

    /// Infallible: `BasisPoints` is already valid by construction.
    pub fn restore(s: PriceListSnapshot) -> PriceList {
        PriceList {
            id: s.id,
            version: s.version,
            currency: s.currency,
            adjustment: s.adjustment,
            events: Recorder::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::Money;
    use uuid::Uuid;

    use super::*;

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn price(amount: i64, currency: Currency) -> Price {
        Price::new(Money::new(amount, currency).unwrap()).unwrap()
    }

    fn eur_list() -> PriceList {
        PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            Currency::Eur,
            BasisPoints::ZERO,
        )
    }

    #[test]
    fn new_keeps_adjustment() {
        let markup = BasisPoints::new(1500).unwrap();
        let list = PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            Currency::Eur,
            markup,
        );
        assert_eq!(list.adjustment(), markup);
    }

    #[test]
    fn set_adjustment_replaces_it() {
        let mut list = eur_list();
        let discount = BasisPoints::new(-2000).unwrap();
        list.set_adjustment(discount);
        assert_eq!(list.adjustment(), discount);
    }

    #[test]
    fn accepts_prices_in_list_currency() {
        let prices = [
            (vid(1), price(1000, Currency::Eur)),
            (vid(2), price(0, Currency::Eur)),
        ];
        assert_eq!(eur_list().check_fixed_prices(&prices), Ok(()));
    }

    #[test]
    fn accepts_no_prices() {
        assert_eq!(eur_list().check_fixed_prices(&[]), Ok(()));
    }

    #[test]
    fn reports_every_mismatch_sorted_by_variant() {
        let prices = [
            (vid(3), price(300, Currency::Rsd)),
            (vid(2), price(200, Currency::Eur)),
            (vid(1), price(100, Currency::Usd)),
        ];
        assert_eq!(
            eur_list().check_fixed_prices(&prices),
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
    fn rejects_duplicate_variant() {
        let prices = [
            (vid(1), price(900, Currency::Eur)),
            (vid(1), price(800, Currency::Eur)),
        ];
        assert_eq!(
            eur_list().check_fixed_prices(&prices),
            Err(PriceListError::DuplicateFixedPrice(vid(1)))
        );
    }

    #[test]
    fn restore_round_trips() {
        let mut list = eur_list();
        list.set_adjustment(BasisPoints::new(-1500).unwrap());
        list.set_version(Version::new(2));
        assert_eq!(PriceList::restore(list.snapshot()), list);
    }
}
