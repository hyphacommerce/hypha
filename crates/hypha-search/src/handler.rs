//! `SearchIndexer` (design note 0019, section 5): the event handler that queues what
//! to rebuild. It only marks; the sync job does the work, so a busy product is rebuilt
//! once however many events it had.

use hypha_core::{EventHandler, StoredEvent};

use crate::{HasSearchQueue, SearchQueue, Target};

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchIndexer;

/// The events search reads, and what each marks.
const PRODUCT_EVENTS: &[&str] = &[
    "product.created",
    "product.updated",
    "product.variant_added",
    "product.variant_updated",
    "product.variant_removed",
];
const CATALOG_PRODUCTS: &[&str] = &["catalog.products_added", "catalog.products_removed"];
const VARIANT_PRICES: &[&str] = &["price_list.prices_set", "price_list.prices_removed"];
const STOCK: &[&str] = &["inventory_item.changed", "inventory_item.deleted"];
/// Rare and wide: which products a market sells, at what price, from where.
const EVERYTHING: &[&str] = &[
    "market.changed",
    "catalog.changed",
    "price_list.changed",
    "location.changed",
];
const SETTINGS: &str = "search_settings.changed";

impl<P: HasSearchQueue> EventHandler<P> for SearchIndexer {
    fn name(&self) -> &'static str {
        "search_index"
    }

    fn wants(&self, event_type: &str) -> bool {
        [
            PRODUCT_EVENTS,
            CATALOG_PRODUCTS,
            VARIANT_PRICES,
            STOCK,
            EVERYTHING,
        ]
        .iter()
        .any(|types| types.contains(&event_type))
            || event_type == SETTINGS
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let targets = targets(event)?;
        if targets.is_empty() {
            return Ok(());
        }
        ports
            .search_queue()
            .mark(&targets)
            .await
            .map_err(|e| e.to_string())
    }
}

/// What `event` asks to rebuild. `Err` when its payload isn't what its type promises.
pub fn targets(event: &StoredEvent) -> Result<Vec<Target>, String> {
    let t = event.event_type.as_str();
    let bad = |what: &str| format!("event {} ({t}) has no {what}", event.id);
    let ids = |field: &str| -> Result<Vec<String>, String> {
        match &event.data[field] {
            serde_json::Value::String(id) => Ok(vec![id.clone()]),
            serde_json::Value::Array(ids) => ids
                .iter()
                .map(|id| id.as_str().map(str::to_string).ok_or_else(|| bad(field)))
                .collect(),
            _ => Err(bad(field)),
        }
    };
    let parse = |field: &str, make: fn(&str) -> Option<Target>| -> Result<Vec<Target>, String> {
        ids(field)?
            .iter()
            .map(|id| make(id).ok_or_else(|| bad(field)))
            .collect()
    };

    if PRODUCT_EVENTS.contains(&t) {
        parse("product", |id| id.parse().ok().map(Target::Product))
    } else if CATALOG_PRODUCTS.contains(&t) {
        parse("products", |id| id.parse().ok().map(Target::Product))
    } else if VARIANT_PRICES.contains(&t) {
        parse("variants", |id| id.parse().ok().map(Target::Variant))
    } else if STOCK.contains(&t) {
        parse("variant", |id| id.parse().ok().map(Target::Variant))
    } else if EVERYTHING.contains(&t) {
        Ok(vec![Target::All])
    } else if t == SETTINGS {
        // Facets are read when searching; only splits change documents.
        let splits = event.data["fields"]
            .as_array()
            .is_some_and(|f| f.iter().any(|f| f == "split_by"));
        Ok(if splits { vec![Target::All] } else { vec![] })
    } else {
        Ok(vec![])
    }
}
