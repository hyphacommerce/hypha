//! Search documents: one per variant, market and language the market offers, with its
//! product's fields in that language, priced and stocked as a shopper in that market sees
//! it. Built by `hypha_search::build_documents`; engines store them as they like.

use hypha_core::{
    AssetId, CategoryId, ChannelId, Language, MarketId, Money, ProductId, Timestamp, VariantId,
};

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

/// One variant of one active product, as a shopper in `market` reading `language` sees
/// it. A read model built by `hypha_search::build_documents`, stored by engines as they
/// like.
///
/// Text a shopper reads (title, variant title, description, `option_labels`) is in
/// `language`; what a filter or a listing names (`options`, the facet values) stays as
/// stored, so a filter means the same in every language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub market: MarketId,
    /// The language of its text: one the market offers.
    pub language: Language,
    pub product: ProductId,
    pub variant: VariantId,
    /// What `group=listing` collapses on: the product ID, followed by the split
    /// options' values when the merchant splits listings (`prod_…|color=Red`).
    pub listing: String,
    /// The channels the product is published to, sorted: the same on every document of the
    /// product.
    pub channels: Vec<ChannelId>,
    /// The product's shown categories and all their ancestors, sorted: a filter on
    /// "Clothing" finds what's in "Shirts" too, and nothing that's only in a hidden
    /// category.
    pub categories: Vec<CategoryId>,
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
    /// Option name and value label as stored, in option order (`("Color", "Red")`): the
    /// facet's key and value, and the listing key's.
    pub options: Vec<(String, String)>,
    /// The same options in `language` (`("Farbe", "Rot")`), one per entry of `options`:
    /// what a shopper reads, and what terms come from.
    pub option_labels: Vec<(String, String)>,
    /// Custom field values a storefront may filter by: the field's full key and one facet
    /// value (`("custom.material", "wool")`), the variant's value where it has one, else
    /// the product's. A `choice` gives its key (labels come from the definition), a list
    /// one entry per item, a boolean `true`/`false`, an integer its digits. Sorted by key,
    /// then in the list's order; no (key, value) twice.
    pub fields: Vec<(String, String)>,
    /// In the market's currency: what one costs.
    pub price: Money,
    /// What `price` is shown against, struck through; only ever above it.
    pub compare_at: Option<Money>,
    /// Whether there's a compare-at price: the `on_sale` facet.
    pub on_sale: bool,
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

/// The fields' terms by weight, for engines that rank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTerms {
    /// Title and variant title.
    pub a: Vec<String>,
    /// Vendor, product type, tags, SKU, option values (as labelled in the language).
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
        for (_, value) in &self.option_labels {
            b.extend(terms::terms(value));
        }
        WeightedTerms {
            a,
            b,
            d: terms::terms(&self.description),
        }
    }

    /// Its facet values, as `FacetKey` strings and values: `option.color` → `Red`,
    /// `vendor` → `Acme`, one `tag` per tag, `on_sale` → `true` or `false`,
    /// `field.custom.material` → `wool`. Keys and values are as stored, whatever the
    /// document's language.
    pub fn facet_values(&self) -> Vec<(String, String)> {
        self.labelled_facet_values()
            .into_iter()
            .map(|(key, value, _)| (key, value))
            .collect()
    }

    /// `facet_values`, each with its label in the document's language: an option
    /// value's from `option_labels`, every other value its own (custom field labels come
    /// from their definitions).
    pub fn labelled_facet_values(&self) -> Vec<(String, String, String)> {
        let mut values = Vec::new();
        let mut plain = |key: crate::FacetKey, value: &str| {
            values.push((key.to_string(), value.to_string(), value.to_string()));
        };
        let mut options = Vec::new();
        for (i, (name, value)) in self.options.iter().enumerate() {
            let label = self
                .option_labels
                .get(i)
                .map_or(value.as_str(), |(_, label)| label.as_str());
            options.push((
                crate::FacetKey::Option(name.to_lowercase()).to_string(),
                value.clone(),
                label.to_string(),
            ));
        }
        if let Some(vendor) = &self.vendor {
            plain(crate::FacetKey::Vendor, vendor);
        }
        if let Some(product_type) = &self.product_type {
            plain(crate::FacetKey::ProductType, product_type);
        }
        for tag in &self.tags {
            plain(crate::FacetKey::Tag, tag);
        }
        plain(crate::FacetKey::OnSale, &self.on_sale.to_string());
        for (key, value) in &self.fields {
            plain(crate::FacetKey::Field(key.clone()), value);
        }
        options.extend(values);
        options
    }
}
