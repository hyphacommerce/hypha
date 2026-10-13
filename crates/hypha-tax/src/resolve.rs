use std::collections::HashMap;

use hypha_core::{Money, MoneyError, ProductId, TaxClassId, TaxRate, TaxRegionId};

use crate::TaxRegion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxContext {
    pub region: Option<TaxRegion>,
    pub prices_include_tax: bool,
    pub product_classes: HashMap<ProductId, TaxClassId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxableLine {
    pub product: ProductId,
    pub amount: Money,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxSource {
    RateTable { region: TaxRegionId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxLine {
    pub rate: TaxRate,
    pub class: Option<TaxClassId>,
    pub amount: Money,
    pub source: TaxSource,
}

pub fn tax_line(ctx: &TaxContext, line: &TaxableLine) -> Result<Option<TaxLine>, MoneyError> {
    let Some(region) = &ctx.region else {
        return Ok(None);
    };
    let (rate, class) = region.rate_for(ctx.product_classes.get(&line.product).copied());
    let amount = if ctx.prices_include_tax {
        rate.tax_in_gross(line.amount)?
    } else {
        rate.tax_on_net(line.amount)?
    };
    Ok(Some(TaxLine {
        rate,
        class,
        amount,
        source: TaxSource::RateTable {
            region: region.id(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use hypha_core::{CountryCode, Currency};
    use uuid::Uuid;

    use super::*;

    fn pid(n: u128) -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(n))
    }
    fn books() -> TaxClassId {
        TaxClassId::from_uuid(Uuid::from_u128(10))
    }
    fn region_id() -> TaxRegionId {
        TaxRegionId::from_uuid(Uuid::from_u128(20))
    }

    fn rate(ppm: u32) -> TaxRate {
        TaxRate::new(ppm).unwrap()
    }

    fn eur(amount: i64) -> Money {
        Money::new(amount, Currency::Eur).unwrap()
    }

    fn croatia(prices_include_tax: bool) -> TaxContext {
        let mut region =
            TaxRegion::new(region_id(), CountryCode::new("HR").unwrap(), rate(250_000));
        region.set_class_rate(books(), rate(50_000));
        TaxContext {
            region: Some(region),
            prices_include_tax,
            product_classes: HashMap::from([(pid(2), books())]),
        }
    }

    fn line(product: u128, amount: i64) -> TaxableLine {
        TaxableLine {
            product: pid(product),
            amount: eur(amount),
        }
    }

    #[test]
    fn adds_default_rate_on_top_when_prices_exclude_tax() {
        assert_eq!(
            tax_line(&croatia(false), &line(1, 1000)),
            Ok(Some(TaxLine {
                rate: rate(250_000),
                class: None,
                amount: eur(250),
                source: TaxSource::RateTable {
                    region: region_id()
                }
            }))
        )
    }

    #[test]
    fn extracts_tax_when_prices_include_it() {
        let taxed = tax_line(&croatia(true), &line(1, 1000)).unwrap().unwrap();
        assert_eq!(taxed.amount, eur(200));
    }

    #[test]
    fn product_in_a_class_pays_the_class_rate() {
        let taxed = tax_line(&croatia(false), &line(2, 1000)).unwrap().unwrap();
        assert_eq!((taxed.rate, taxed.class), (rate(50_000), Some(books())));
        assert_eq!(taxed.amount, eur(50));
    }

    #[test]
    fn no_region_means_no_tax() {
        let ctx = TaxContext {
            region: None,
            ..croatia(false)
        };
        assert_eq!(tax_line(&ctx, &line(1, 1000)), Ok(None));
    }
}
