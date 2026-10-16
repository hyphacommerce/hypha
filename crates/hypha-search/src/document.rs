//! Search documents (design note 0019, section 3): one per variant per market, with
//! its product's fields, priced and stocked as a shopper in that market sees it.

use std::collections::BTreeMap;

use hypha_core::{AssetId, MarketId, Money, ProductId, Quantity, Stamped, Timestamp, VariantId};
use hypha_inventory::{Availability, StockContext};
use hypha_pricing::{PricingContext, resolve_price};
use hypha_products::{Product, ProductStatus};

use crate::{SearchSettings, terms};

/// The lowest and highest price over a set of documents, in the market's currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceRange {
    pub min: Money,
    pub max: Money,
}

impl PriceRange {
    fn of(price: Money) -> Self {
        Self {
            min: price,
            max: price,
        }
    }

    fn widen(&mut self, price: Money) {
        if price.amount() < self.min.amount() {
            self.min = price;
        }
        if price.amount() > self.max.amount() {
            self.max = price;
        }
    }
}

/// One variant of one active product, as a shopper in `market` sees it. A read model
/// built by `build_documents`, stored by engines as they like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub market: MarketId,
    pub product: ProductId,
    pub variant: VariantId,
    /// What `group=listing` collapses on: the product ID, followed by the split
    /// options' values when the merchant splits listings (`prod_…|color=Red`).
    pub listing: String,
    pub handle: String,
    pub title: String,
    /// "M / Blue"; `None` for a product without options.
    pub variant_title: Option<String>,
    /// The description's text, markup removed.
    pub description: String,
    pub vendor: Option<String>,
    pub product_type: Option<String>,
    pub tags: Vec<String>,
    pub sku: Option<String>,
    /// Option name and value label, in option order (`("Color", "Red")`).
    pub options: Vec<(String, String)>,
    /// In the market's currency.
    pub price: Money,
    /// Over the product's documents in this market, and its listing's.
    pub product_price: PriceRange,
    pub listing_price: PriceRange,
    pub available: bool,
    /// The variant's image, else the product's featured item.
    pub image: Option<AssetId>,
    /// The product's: "newest" sorts by it.
    pub created_at: Timestamp,
    /// The variant's place in its product: a group's representative is the first.
    pub position: u32,
}

/// The fields' terms by weight, for engines that rank (design note 0019, section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTerms {
    /// Title and variant title.
    pub a: Vec<String>,
    /// Vendor, product type, tags, SKU, option values.
    pub b: Vec<String>,
    /// Description.
    pub d: Vec<String>,
}

impl WeightedTerms {
    pub fn all(&self) -> Vec<String> {
        self.a
            .iter()
            .chain(&self.b)
            .chain(&self.d)
            .cloned()
            .collect()
    }
}

impl SearchDocument {
    pub fn terms(&self) -> WeightedTerms {
        let mut a = terms::terms(&self.title);
        if let Some(t) = &self.variant_title {
            a.extend(terms::terms(t));
        }
        let mut b = Vec::new();
        for text in [&self.vendor, &self.product_type, &self.sku]
            .into_iter()
            .flatten()
        {
            b.extend(terms::terms(text));
        }
        for tag in &self.tags {
            b.extend(terms::terms(tag));
        }
        for (_, value) in &self.options {
            b.extend(terms::terms(value));
        }
        WeightedTerms {
            a,
            b,
            d: terms::terms(&self.description),
        }
    }

    /// Its facet values, as `FacetKey` strings and values: `option.color` → `Red`,
    /// `vendor` → `Acme`, one `tag` per tag.
    pub fn facet_values(&self) -> Vec<(String, String)> {
        let mut values = Vec::new();
        for (name, value) in &self.options {
            values.push((
                crate::FacetKey::Option(name.to_lowercase()).to_string(),
                value.clone(),
            ));
        }
        if let Some(vendor) = &self.vendor {
            values.push((crate::FacetKey::Vendor.to_string(), vendor.clone()));
        }
        if let Some(product_type) = &self.product_type {
            values.push((
                crate::FacetKey::ProductType.to_string(),
                product_type.clone(),
            ));
        }
        for tag in &self.tags {
            values.push((crate::FacetKey::Tag.to_string(), tag.clone()));
        }
        values
    }
}

/// `product`'s documents in the context's market: one per variant whose price
/// resolves there, none if the product isn't active or the market's catalog doesn't
/// include it. `pricing` and `stock` must have been loaded for this product's variants
/// (`hypha_pricing::load_pricing`, `StockContext::load`), the same way a cart is, so
/// search and cart agree on prices and stock.
pub fn build_documents(
    product: &Product,
    pricing: &PricingContext,
    stock: &StockContext,
    settings: &SearchSettings,
) -> Vec<SearchDocument> {
    if product.status() != ProductStatus::Active {
        return Vec::new();
    }
    let market = pricing.market.id();
    let created_at = product
        .stamps()
        .map_or(Timestamp::from_unix_micros(0), |s| s.created_at);
    let description = product.description().map(|d| d.text()).unwrap_or_default();
    let split: Vec<String> = settings
        .split_by()
        .iter()
        .map(|name| name.to_lowercase())
        .collect();

    let mut docs: Vec<SearchDocument> = Vec::new();
    for (position, variant) in product.variants().iter().enumerate() {
        let Ok(price) = resolve_price(pricing, product.id(), variant.id(), variant.base_price())
        else {
            continue;
        };
        let options: Vec<(String, String)> = product
            .options()
            .iter()
            .zip(variant.selections())
            .filter_map(|(option, selected)| {
                option
                    .values()
                    .iter()
                    .find(|v| v.id() == *selected)
                    .map(|v| (option.name().to_string(), v.label().to_string()))
            })
            .collect();
        let price = price.money();
        docs.push(SearchDocument {
            market,
            product: product.id(),
            variant: variant.id(),
            listing: listing_key(product.id(), &options, &split),
            handle: product.handle().as_str().to_string(),
            title: product.title().to_string(),
            variant_title: product.variant_title(variant.id()),
            description: description.clone(),
            vendor: product.vendor().map(str::to_string),
            product_type: product.product_type().map(str::to_string),
            tags: product.tags().to_vec(),
            sku: variant.sku().map(str::to_string),
            options,
            price,
            product_price: PriceRange::of(price),
            listing_price: PriceRange::of(price),
            available: !matches!(
                stock.availability(variant.id(), Quantity::ONE),
                Availability::OutOfStock { .. }
            ),
            image: variant.media().or(product.featured_media()),
            created_at,
            position: u32::try_from(position).unwrap_or(u32::MAX),
        });
    }

    // Price ranges over the product's and each listing's documents.
    let Some(first) = docs.first() else {
        return docs;
    };
    let mut product_price = PriceRange::of(first.price);
    let mut listing_prices: BTreeMap<String, PriceRange> = BTreeMap::new();
    for doc in &docs {
        product_price.widen(doc.price);
        listing_prices
            .entry(doc.listing.clone())
            .and_modify(|r| r.widen(doc.price))
            .or_insert(PriceRange::of(doc.price));
    }
    for doc in &mut docs {
        doc.product_price = product_price;
        if let Some(range) = listing_prices.get(&doc.listing) {
            doc.listing_price = *range;
        }
    }
    docs
}

/// The product ID, then `|name=value` for each split option the product has, in the
/// settings' order (names lowercased; values as labelled).
fn listing_key(product: ProductId, options: &[(String, String)], split: &[String]) -> String {
    let mut key = product.to_string();
    for name in split {
        if let Some((_, value)) = options.iter().find(|(n, _)| n.to_lowercase() == *name) {
            key.push('|');
            key.push_str(name);
            key.push('=');
            key.push_str(value);
        }
    }
    key
}
