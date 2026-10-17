//! `SearchIndexer` (design note 0019, section 5): the event handler that queues what
//! to rebuild. It only marks; the sync job does the work, so a busy product is rebuilt
//! once however many events it had.

use hypha_core::{EventHandler, StoredEvent};

use hypha_search_index::{HasSearchQueue, SearchQueue, Target};

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
/// Which channels sell a product (design note 0020); a channel's name isn't in documents.
const PUBLICATIONS: &[&str] = &["channel.products_published", "channel.products_unpublished"];
/// Which categories a product is in (design note 0022); a deleted category names its
/// products too.
const CATEGORY_PRODUCTS: &[&str] = &[
    "category.products_added",
    "category.products_removed",
    "category.deleted",
];
/// A moved category changes the ancestors of everything below it. Names, handles and
/// visibility aren't in documents (a hidden category is refused at the route, CA4).
const CATEGORY_MOVED: (&str, &str) = ("category.changed", "parent");
const VARIANT_PRICES: &[&str] = &["price_list.prices_set", "price_list.prices_removed"];
const STOCK: &[&str] = &["inventory_item.changed", "inventory_item.deleted"];
/// Rare and wide: which products a market sells, at what price, from where.
const EVERYTHING: &[&str] = &["market.changed", "catalog.changed", "price_list.changed"];
/// Everything too, but only when the field named changed: a location's `markets` decide
/// where its stock counts; the settings' `split_by` changes every listing key.
const WHEN_FIELD: &[(&str, &str)] = &[
    ("location.changed", "markets"),
    ("search_settings.changed", "split_by"),
];

impl<P: HasSearchQueue> EventHandler<P> for SearchIndexer {
    fn name(&self) -> &'static str {
        "search_index"
    }

    fn wants(&self, event_type: &str) -> bool {
        [
            PRODUCT_EVENTS,
            CATALOG_PRODUCTS,
            PUBLICATIONS,
            CATEGORY_PRODUCTS,
            VARIANT_PRICES,
            STOCK,
            EVERYTHING,
        ]
        .iter()
        .any(|types| types.contains(&event_type))
            || event_type == CATEGORY_MOVED.0
            || WHEN_FIELD.iter().any(|(t, _)| *t == event_type)
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
    } else if CATALOG_PRODUCTS.contains(&t)
        || PUBLICATIONS.contains(&t)
        || CATEGORY_PRODUCTS.contains(&t)
    {
        parse("products", |id| id.parse().ok().map(Target::Product))
    } else if VARIANT_PRICES.contains(&t) {
        parse("variants", |id| id.parse().ok().map(Target::Variant))
    } else if STOCK.contains(&t) {
        parse("variant", |id| id.parse().ok().map(Target::Variant))
    } else if t == CATEGORY_MOVED.0 {
        let fields = event.data["fields"]
            .as_array()
            .ok_or_else(|| bad("fields"))?;
        if fields.iter().any(|f| f == CATEGORY_MOVED.1) {
            parse("category", |id| id.parse().ok().map(Target::Category))
        } else {
            Ok(vec![])
        }
    } else if EVERYTHING.contains(&t) {
        Ok(vec![Target::All])
    } else if let Some((_, field)) = WHEN_FIELD.iter().find(|(w, _)| *w == t) {
        let fields = event.data["fields"]
            .as_array()
            .ok_or_else(|| bad("fields"))?;
        Ok(if fields.iter().any(|f| f == field) {
            vec![Target::All]
        } else {
            vec![]
        })
    } else {
        Ok(vec![])
    }
}
