//! Search documents (design note 0019, section 3): one per variant per market, with
//! its product's fields, priced and stocked as a shopper in that market sees it. Built
//! by `hypha_search::build_documents`; engines store them as they like.

use hypha_core::{AssetId, ChannelId, MarketId, Money, ProductId, Timestamp, VariantId};

use crate::terms;

/// The lowest and highest price over a set of documents, in the market's currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceRange {
    pub min: Money,
    pub max: Money,
}

impl PriceRange {
    /// A range of one price.
    pub fn of(price: Money) -> Self {
        Self {
            min: price,
            max: price,
        }
    }

    /// Widens the range to include `price`.
    pub fn widen(&mut self, price: Money) {
        if price.amount() < self.min.amount() {
            self.min = price;
        }
        if price.amount() > self.max.amount() {
            self.max = price;
        }
    }
}

/// One variant of one active product, as a shopper in `market` sees it. A read model
/// built by `hypha_search::build_documents`, stored by engines as they like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub market: MarketId,
    pub product: ProductId,
    pub variant: VariantId,
    /// What `group=listing` collapses on: the product ID, followed by the split
    /// options' values when the merchant splits listings (`prod_…|color=Red`).
    pub listing: String,
    /// The channels the product is published to, sorted (design note 0020): the same on
    /// every document of the product.
    pub channels: Vec<ChannelId>,
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
