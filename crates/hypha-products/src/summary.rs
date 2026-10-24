//! What a list of products shows: no options or variants, so a page costs the same however
//! many variants its products have. A read model, not an aggregate: adapters may compute it
//! in storage (Postgres aggregates it in SQL), and the repository contract checks it equals
//! `Product::summary`.

use hypha_core::{AssetId, Currency, Price, ProductId, Stamped, Stamps, VariantId};

use crate::{Handle, Product, ProductStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSummary {
    pub id: ProductId,
    pub handle: Handle,
    pub title: String,
    pub status: ProductStatus,
    pub vendor: Option<String>,
    pub product_type: Option<String>,
    pub variant_count: usize,
    /// The lowest and highest base price in each currency the variants are priced in,
    /// ordered by currency code (variants may be priced in different currencies).
    pub price_ranges: Vec<PriceRange>,
    /// The first item of the product's media.
    pub featured_media: Option<AssetId>,
    /// When the product was stored and last changed.
    pub stamps: Option<Stamps>,
}

/// The lowest and highest base price among variants priced in one currency; `min` and
/// `max` are in that currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceRange {
    pub min: Price,
    pub max: Price,
}

/// A variant in a list of variants across products: what a price grid shows, with its
/// product's title and handle. Adapters may compute it in storage; the repository contract
/// checks it equals what `Product::variant_summaries` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSummary {
    pub id: VariantId,
    pub product: ProductId,
    pub product_title: String,
    pub product_handle: Handle,
    /// Its index in the product's variants.
    pub position: u32,
    pub sku: Option<String>,
    /// `Product::variant_title`.
    pub title: Option<String>,
    pub price: Price,
    pub compare_at_price: Option<Price>,
}

impl Product {
    /// Each variant's summary, in the product's order.
    pub fn variant_summaries(&self) -> Vec<VariantSummary> {
        (0u32..)
            .zip(self.variants())
            .map(|(position, v)| VariantSummary {
                id: v.id(),
                product: self.id(),
                product_title: self.title().to_string(),
                product_handle: self.handle().clone(),
                position,
                sku: v.sku().map(str::to_string),
                title: self.variant_title(v.id()),
                price: v.base_price(),
                compare_at_price: v.compare_at_price(),
            })
            .collect()
    }

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
            vendor: self.vendor().map(str::to_string),
            product_type: self.product_type().map(str::to_string),
            variant_count: self.variants().len(),
            price_ranges,
            featured_media: self.featured_media(),
            stamps: self.stamps(),
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
        let eur = |a| price(a, Currency::EUR);
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
            price(2000, Currency::USD),
            price(1500, Currency::EUR),
            price(1000, Currency::USD),
        ])
        .summary();
        assert_eq!(
            summary.price_ranges,
            vec![
                PriceRange {
                    min: price(1500, Currency::EUR),
                    max: price(1500, Currency::EUR)
                },
                PriceRange {
                    min: price(1000, Currency::USD),
                    max: price(2000, Currency::USD)
                },
            ]
        );
    }
}
