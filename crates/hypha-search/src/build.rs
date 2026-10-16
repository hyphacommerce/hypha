//! Building search documents (design note 0019, section 3) from a product, its prices
//! and its stock in one market.

use std::collections::BTreeMap;

use hypha_core::{ChannelId, ProductId, Quantity, Stamped, Timestamp};
use hypha_inventory::{Availability, StockContext};
use hypha_pricing::{PricingContext, resolve_price};
use hypha_products::{Product, ProductStatus};
use hypha_search_index::{PriceRange, SearchDocument, SearchSettings};

/// `product`'s documents in the context's market: one per variant whose price
/// resolves there, none if the product isn't active or the market's catalog doesn't
/// include it. `pricing` and `stock` must have been loaded for this product's variants
/// (`hypha_pricing::load_pricing`, `StockContext::load`), the same way a cart is, so
/// search and cart agree on prices and stock.
/// `channels` are the ones the product is published to (`ChannelRepo::channels_of`).
pub fn build_documents(
    product: &Product,
    pricing: &PricingContext,
    stock: &StockContext,
    settings: &SearchSettings,
    channels: &[ChannelId],
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
            channels: channels.to_vec(),
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
