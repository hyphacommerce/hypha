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
    // Its documents go: the product doesn't load any more (design note 0033, 6.3).
    "product.deleted",
];
/// A product's or its variants' custom field values (design note 0033, §9): its facet
/// fields.
const PRODUCT_FIELDS: &[&str] = &["product.custom_fields_updated"];
/// A product or variant field definition's change to who reads it or whether it
/// filters, or its deletion (with its values), changes which values every document
/// holds: everything. Others (names, validations, positions) aren't in documents.
const FIELD_DEFINITIONS: &[(&str, &[&str])] = &[
    (
        "custom_field_definition.updated",
        &["access", "capabilities"],
    ),
    ("custom_field_definition.deleted", &[]),
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
const VARIANT_PRICES: &[&str] = &["price_list.prices_set", "price_list.prices_removed"];
const STOCK: &[&str] = &["inventory_item.changed", "inventory_item.deleted"];
/// Rare and wide: which products a market sells, at what price, from where.
const EVERYTHING: &[&str] = &["catalog.changed", "price_list.changed"];
/// What to rebuild when one of the fields named changed (the event's `fields`):
/// - a location's `markets` decide where its stock counts, the settings' `split_by`
///   changes every listing key, and a market's countries, currency and tax rule decide
///   every price there: everything;
/// - a category's `parent` or `visible` change which categories the products below it
///   are listed under (design note 0022, CA3, CA4): that subtree.
///
/// Names and the like aren't in documents and rebuild nothing.
const WHEN_FIELD: &[(&str, &[&str], Rebuild)] = &[
    ("location.changed", &["markets"], Rebuild::All),
    ("search_settings.changed", &["split_by"], Rebuild::All),
    (
        "market.changed",
        &["created", "countries", "currency", "prices_include_tax"],
        Rebuild::All,
    ),
    (
        "category.changed",
        &["parent", "visible"],
        Rebuild::Category,
    ),
];

/// What a `WHEN_FIELD` rule marks.
#[derive(Clone, Copy)]
enum Rebuild {
    All,
    /// The event's `category` and everything below it.
    Category,
}

impl<P: HasSearchQueue> EventHandler<P> for SearchIndexer {
    fn name(&self) -> &'static str {
        "search_index"
    }

    fn wants(&self, event_type: &str) -> bool {
        [
            PRODUCT_EVENTS,
            PRODUCT_FIELDS,
            CATALOG_PRODUCTS,
            PUBLICATIONS,
            CATEGORY_PRODUCTS,
            VARIANT_PRICES,
            STOCK,
            EVERYTHING,
        ]
        .iter()
        .any(|types| types.contains(&event_type))
            || WHEN_FIELD.iter().any(|(t, ..)| *t == event_type)
            || FIELD_DEFINITIONS.iter().any(|(t, _)| *t == event_type)
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

    if PRODUCT_EVENTS.contains(&t) || PRODUCT_FIELDS.contains(&t) {
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
    } else if EVERYTHING.contains(&t) {
        Ok(vec![Target::All])
    } else if let Some((_, wanted)) = FIELD_DEFINITIONS.iter().find(|(w, _)| *w == t) {
        let owner = event.data["owner_type"]
            .as_str()
            .ok_or_else(|| bad("owner_type"))?;
        if !matches!(owner, "product" | "variant") {
            return Ok(vec![]);
        }
        // Every field of a deletion counts (`wanted` is empty).
        let changed = wanted.is_empty()
            || match &event.data["fields"] {
                serde_json::Value::Array(fields) => {
                    fields.iter().any(|f| wanted.iter().any(|w| f == *w))
                }
                _ => return Err(bad("fields")),
            };
        Ok(if changed { vec![Target::All] } else { vec![] })
    } else if let Some((_, wanted, rebuild)) = WHEN_FIELD.iter().find(|(w, ..)| *w == t) {
        // Events written before `fields` existed (`market.changed`) rebuild as if every
        // field changed: the safe guess.
        let changed = match &event.data["fields"] {
            serde_json::Value::Null => true,
            serde_json::Value::Array(fields) => {
                fields.iter().any(|f| wanted.iter().any(|w| f == *w))
            }
            _ => return Err(bad("fields")),
        };
        if !changed {
            return Ok(vec![]);
        }
        match rebuild {
            Rebuild::All => Ok(vec![Target::All]),
            Rebuild::Category => parse("category", |id| id.parse().ok().map(Target::Category)),
        }
    } else {
        Ok(vec![])
    }
}
