//! What a list of products shows (design note 0012, L7): no options or variants, so a
//! page costs the same however many variants its products have. A read model, not an
//! aggregate: adapters may compute it in storage (Postgres aggregates it in SQL), and
//! the repository contract checks it equals `Product::summary`.

use hypha_core::{AssetId, Currency, Price, ProductId};

use crate::{Handle, Product, ProductStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSummary {
    pub id: ProductId,
    pub handle: Handle,
    pub title: String,
    pub status: ProductStatus,
    pub variant_count: usize,
    /// The lowest and highest base price in each currency the variants are priced in,
    /// ordered by currency code (variants may be priced in different currencies).
    pub price_ranges: Vec<PriceRange>,
    /// The first item of the product's media (design note 0014).
    pub featured_media: Option<AssetId>,
}

/// The lowest and highest base price among variants priced in one currency; `min` and
/// `max` are in that currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceRange {
    pub min: Price,
    pub max: Price,
}

impl Product {
    pub fn summary(&self) -> ProductSummary {
        let mut price_ranges: Vec<PriceRange> = Vec::new();
        for variant in self.variants() {
            let price = variant.base_price();
            match price_ranges
                .iter_mut()
                .find(|r| currency(r.min) == currency(price))
            {
                Some(range) => {
                    if price.money().amount() < range.min.money().amount() {
                        range.min = price;
                    }
                    if price.money().amount() > range.max.money().amount() {
                        range.max = price;
                    }
                }
                None => price_ranges.push(PriceRange {
                    min: price,
                    max: price,
                }),
            }
        }
        price_ranges.sort_by_key(|r| currency(r.min).as_str());
        ProductSummary {
            id: self.id(),
            handle: self.handle().clone(),
            title: self.title().to_string(),
            status: self.status(),
            variant_count: self.variants().len(),
            price_ranges,
            featured_media: self.featured_media(),
        }
    }
}

fn currency(price: Price) -> Currency {
    price.money().currency()
}

#[cfg(test)]
mod tests {
    use hypha_core::{Money, OptionId, OptionValueId, VariantId};
    use uuid::Uuid;

    use super::*;
    use crate::{NewVariant, OptionValue, ProductOption};

    fn price(amount: i64, currency: Currency) -> Price {
        Price::new(Money::new(amount, currency).unwrap()).unwrap()
    }

    /// A product with one option and a variant per price.
    fn priced(prices: &[Price]) -> Product {
        let values: Vec<OptionValue> = (0..prices.len())
            .map(|n| {
                OptionValue::new(
                    OptionValueId::from_uuid(Uuid::from_u128(100 + n as u128)),
                    format!("V{n}"),
                )
            })
            .collect();
        let option = ProductOption::new(
            OptionId::from_uuid(Uuid::from_u128(10)),
            "Size".to_string(),
            values.clone(),
        );
        let variant = |n: usize| NewVariant {
            id: VariantId::from_uuid(Uuid::from_u128(200 + n as u128)),
            sku: None,
            price: prices[n],
            selections: vec![values[n].id()],
        };
        let mut product = Product::new_configurable(
            ProductId::from_uuid(Uuid::from_u128(1)),
            Handle::new("shirt").unwrap(),
            "Shirt".to_string(),
            vec![option],
            variant(0),
        )
        .unwrap();
        for n in 1..prices.len() {
            product.add_variant(variant(n)).unwrap();
        }
        product
    }

    #[test]
    fn a_summary_counts_variants_and_spans_their_prices() {
        let eur = |a| price(a, Currency::Eur);
        let summary = priced(&[eur(1500), eur(900), eur(1700)]).summary();
        assert_eq!(summary.variant_count, 3);
        assert_eq!(
            summary.price_ranges,
            vec![PriceRange {
                min: eur(900),
                max: eur(1700)
            }]
        );
    }

    #[test]
    fn a_summary_has_one_price_range_per_currency_by_code() {
        let summary = priced(&[
            price(2000, Currency::Usd),
            price(1500, Currency::Eur),
            price(1000, Currency::Usd),
        ])
        .summary();
        assert_eq!(
            summary.price_ranges,
            vec![
                PriceRange {
                    min: price(1500, Currency::Eur),
                    max: price(1500, Currency::Eur)
                },
                PriceRange {
                    min: price(1000, Currency::Usd),
                    max: price(2000, Currency::Usd)
                },
            ]
        );
    }
}
