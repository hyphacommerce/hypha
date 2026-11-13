//! Building search documents from a product, its prices and its stock in one market, in
//! one language.

use std::collections::BTreeMap;

use hypha_core::{
    CategoryId, ChannelId, Description, Language, ProductId, Quantity, Stamped, Timestamp,
};
use hypha_inventory::{Availability, StockContext};
use hypha_pricing::{PricingContext, VariantPrices, resolve_price};
use hypha_products::{Product, ProductStatus};
use hypha_search_index::{PriceRange, SearchDocument, SearchSettings};
use hypha_translations::{Overlay, ResourceKind, ResourceRef, fields::product as keys};

use crate::fields::FacetFields;

/// Where a product shows, the same in every market and language: the channels it's
/// published to (`ChannelRepo::channels_of`), its categories with all their ancestors,
/// sorted, and its and its variants' storefront-visible facet fields
/// (`crate::fields::load_facet_fields`): a variant's value wins over its product's for the
/// same key.
#[derive(Debug, Clone, Copy)]
pub struct Placement<'a> {
    pub channels: &'a [ChannelId],
    pub categories: &'a [CategoryId],
    pub fields: &'a FacetFields,
}

/// The language documents are written in, with the product's translations into it
/// (`Overlay::none()` for the store's own language). A field without a translation is
/// written as stored.
#[derive(Debug, Clone, Copy)]
pub struct InLanguage<'a> {
    pub language: &'a Language,
    pub overlay: &'a Overlay,
}

/// `product`'s documents in the context's market and `text`'s language: one per variant
/// whose price resolves there, none if the product isn't active or the market's catalog
/// doesn't include it. `pricing` and `stock` must have been loaded for this product's
/// variants (`hypha_pricing::load_pricing`, `StockContext::load`), the same way a cart is,
/// so search and cart agree on prices and stock.
///
/// Its title, description, option names and value labels and the variant title joined
/// from them are in the language; option values as facets and listing keys stay as
/// stored, so a filter or a split listing is the same in every language.
pub fn build_documents(
    product: &Product,
    pricing: &PricingContext,
    stock: &StockContext,
    settings: &SearchSettings,
    placement: Placement<'_>,
    text: InLanguage<'_>,
) -> Vec<SearchDocument> {
    if product.status() != ProductStatus::Active {
        return Vec::new();
    }
    let market = pricing.market.id();
    let created_at = product
        .stamps()
        .map_or(Timestamp::from_unix_micros(0), |s| s.created_at);
    let resource = ResourceRef::new(ResourceKind::Product, product.id().as_uuid());
    let overlay = text.overlay;
    // A translated description is sanitised HTML, as stored descriptions are.
    let description = overlay
        .text(resource, keys::DESCRIPTION)
        .filter(|_| product.description().is_some())
        .and_then(|html| Description::stored(html.to_string()).ok().flatten())
        .or_else(|| product.description().cloned())
        .map(|d| d.text())
        .unwrap_or_default();
    let title = overlay.text_or(resource, keys::TITLE, product.title());
    let split: Vec<String> = settings
        .split_by()
        .iter()
        .map(|name| name.to_lowercase())
        .collect();

    let mut docs: Vec<SearchDocument> = Vec::new();
    for (position, variant) in product.variants().iter().enumerate() {
        let prices = VariantPrices {
            product: product.id(),
            variant: variant.id(),
            base: variant.base_price(),
            compare_at: variant.compare_at_price(),
        };
        // Listings show the price of one.
        let Ok(resolved) = resolve_price(pricing, prices, Quantity::ONE) else {
            continue;
        };
        let selected: Vec<(&hypha_products::ProductOption, &hypha_products::OptionValue)> = product
            .options()
            .iter()
            .zip(variant.selections())
            .filter_map(|(option, selected)| {
                option
                    .values()
                    .iter()
                    .find(|v| v.id() == *selected)
                    .map(|v| (option, v))
            })
            .collect();
        let options: Vec<(String, String)> = selected
            .iter()
            .map(|(option, value)| (option.name().to_string(), value.label().to_string()))
            .collect();
        let option_labels: Vec<(String, String)> = selected
            .iter()
            .map(|(option, value)| {
                let name =
                    overlay.text_or(resource, &keys::option_name(option.id()), option.name());
                let label = overlay.text_or(
                    resource,
                    &keys::option_value_label(value.id()),
                    value.label(),
                );
                (name.to_string(), label.to_string())
            })
            .collect();
        let price = resolved.price.money();
        docs.push(SearchDocument {
            market,
            language: text.language.clone(),
            product: product.id(),
            variant: variant.id(),
            listing: listing_key(product.id(), &options, &split),
            channels: placement.channels.to_vec(),
            categories: placement.categories.to_vec(),
            handle: product.handle().as_str().to_string(),
            title: title.to_string(),
            // Joined as `Product::variant_title` joins it, from the labels shown.
            variant_title: hypha_products::join_variant_title(
                option_labels.iter().map(|(_, label)| label.as_str()),
            ),
            description: description.clone(),
            vendor: product.vendor().map(str::to_string),
            product_type: product.product_type().map(str::to_string),
            tags: product.tags().to_vec(),
            sku: variant.sku().map(str::to_string),
            options,
            option_labels,
            fields: placement.fields.for_variant(variant.id()),
            price,
            compare_at: resolved.compare_at.map(|c| c.money()),
            on_sale: resolved.on_sale(),
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
