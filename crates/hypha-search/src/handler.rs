//! `SearchIndexer`: the event handler that queues what to rebuild. It only marks; the sync
//! job does the work, so a busy product is rebuilt once however many events it had.

use hypha_core::{EventHandler, PriceListId, RepoError, StoredEvent};
use hypha_markets::{AppliesTo, CatalogRepo, HasCatalogs, HasPriceLists, PriceListRepo};
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
    // Its documents go: the product doesn't load any more.
    "product.deleted",
];
/// A product's or its variants' custom field values: its facet fields.
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
/// Which channels sell a product; a channel's name isn't in documents.
const PUBLICATIONS: &[&str] = &["channel.products_published", "channel.products_unpublished"];
/// Which categories a product is in; a deleted category names its products too.
const CATEGORY_PRODUCTS: &[&str] = &[
    "category.products_added",
    "category.products_removed",
    "category.deleted",
];
const VARIANT_PRICES: &[&str] = &["price_list.prices_set", "price_list.prices_removed"];
const STOCK: &[&str] = &["inventory_item.changed", "inventory_item.deleted"];
/// A list's window opened or closed: what it prices changed in its catalogs' markets, as
/// when it's edited. One on no catalog prices nothing.
const WINDOWS: &[&str] = &["price_list.started", "price_list.ended"];
/// Lists joined or left a catalog: what they price there changed. (`catalog.changed`,
/// recorded beside them and when a catalog is made, empty, asks nothing more.)
const CATALOG_LISTS: &[&str] = &["catalog.price_lists_added", "catalog.price_lists_removed"];
/// A list's change to these changes what it prices or how (its name and description
/// don't).
const LIST_PRICING: &[&str] = &[
    "adjustment",
    "kind",
    "status",
    "starts_at",
    "ends_at",
    "compare_at",
    "audience",
];
/// Past this many fixed prices a list's variants aren't marked one by one: everything is.
pub const MAX_VARIANT_MARKS: u32 = 1000;
/// What to rebuild when one of the fields named changed (the event's `fields`):
/// - a location's `markets` decide where its stock counts, the settings' `split_by`
///   changes every listing key, and a market's countries, currency and tax rule decide
///   every price there: everything;
/// - a category's `parent` or `visible` change which categories the products below it are
///   listed under: that subtree.
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

impl<P: HasSearchQueue + HasPriceLists + HasCatalogs> EventHandler<P> for SearchIndexer {
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
            WINDOWS,
            CATALOG_LISTS,
        ]
        .iter()
        .any(|types| types.contains(&event_type))
            || event_type == "price_list.changed"
            || WHEN_FIELD.iter().any(|(t, ..)| *t == event_type)
            || FIELD_DEFINITIONS.iter().any(|(t, _)| *t == event_type)
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let targets = marks(ports, event).await?;
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

/// What `event` asks to rebuild, reading what its price lists reach from storage. `Err`
/// when its payload isn't what its type promises, or storage fails.
pub async fn marks<P: HasPriceLists + HasCatalogs>(
    ports: &P,
    event: &StoredEvent,
) -> Result<Vec<Target>, String> {
    match list_reach(event)? {
        Some(reach) => reach.targets(ports).await.map_err(|e| e.to_string()),
        None => targets(event),
    }
}

/// Price lists whose pricing changed, for a rebuild of what they reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListReach {
    lists: Vec<PriceListId>,
    /// Only when a catalog holds them (an edit; a window's or a catalog's event says).
    if_held: bool,
    /// Their audience changed: a list for customer groups may have priced for everyone.
    audience_changed: bool,
    /// What the adjustment reached before the change, when that changed.
    previously: Option<AppliesTo>,
}

impl ListReach {
    /// Each list's fixed-priced variants (everything, past `MAX_VARIANT_MARKS`) and what
    /// its adjustment reaches now and reached before. Search documents price for no
    /// customer group, so a list for groups marks nothing unless its audience changed. A
    /// list that's gone can't say what it priced: everything.
    pub async fn targets<P: HasPriceLists + HasCatalogs>(
        &self,
        ports: &P,
    ) -> Result<Vec<Target>, RepoError> {
        let mut marks = Vec::new();
        for &id in &self.lists {
            if self.if_held && ports.catalogs().holding_price_list(id).await?.is_empty() {
                continue;
            }
            let Some(list) = ports.price_lists().get(id).await? else {
                return Ok(vec![Target::All]);
            };
            if !list.audience().is_everyone() && !self.audience_changed {
                continue;
            }
            let everything = list
                .adjustment()
                .is_some_and(|a| reach(a.applies_to(), &mut marks))
                | self
                    .previously
                    .as_ref()
                    .is_some_and(|a| reach(a, &mut marks));
            if everything {
                return Ok(vec![Target::All]);
            }
            let variants = ports
                .price_lists()
                .priced_variants(id, MAX_VARIANT_MARKS + 1)
                .await?;
            if variants.len() > MAX_VARIANT_MARKS as usize {
                return Ok(vec![Target::All]);
            }
            marks.extend(variants.into_iter().map(Target::Variant));
        }
        Ok(marks)
    }
}

/// Marks what `applies_to` reaches; `true` when that's every product.
fn reach(applies_to: &AppliesTo, marks: &mut Vec<Target>) -> bool {
    match applies_to {
        AppliesTo::All => return true,
        AppliesTo::Products(products) => marks.extend(products.iter().map(|p| Target::Product(*p))),
        AppliesTo::Categories(categories) => {
            marks.extend(categories.iter().map(|c| Target::Category(*c)));
        }
    }
    false
}

/// The price lists `event` changed the pricing of, if it's such an event; `Ok(None)` for
/// any other.
pub fn list_reach(event: &StoredEvent) -> Result<Option<ListReach>, String> {
    let t = event.event_type.as_str();
    let bad = |what: &str| format!("event {} ({t}) has no {what}", event.id);
    let list_ids = |field: &str| -> Result<Vec<PriceListId>, String> {
        let one = |v: &serde_json::Value| v.as_str().and_then(|id| id.parse().ok());
        match &event.data[field] {
            serde_json::Value::Array(ids) => ids
                .iter()
                .map(|id| one(id).ok_or_else(|| bad(field)))
                .collect(),
            id => Ok(vec![one(id).ok_or_else(|| bad(field))?]),
        }
    };
    let none = ListReach {
        lists: Vec::new(),
        if_held: false,
        audience_changed: false,
        previously: None,
    };
    if WINDOWS.contains(&t) {
        let held = match &event.data["catalogs"] {
            serde_json::Value::Array(catalogs) => !catalogs.is_empty(),
            _ => return Err(bad("catalogs")),
        };
        return Ok(Some(if held {
            ListReach {
                lists: list_ids("price_list")?,
                ..none
            }
        } else {
            none
        }));
    }
    if CATALOG_LISTS.contains(&t) {
        return Ok(Some(ListReach {
            lists: list_ids("price_lists")?,
            ..none
        }));
    }
    if t != "price_list.changed" {
        return Ok(None);
    }
    // Events written before `fields` existed (a creation or a new adjustment) count as
    // every field changed: the safe guess.
    let fields: Vec<&str> = match &event.data["fields"] {
        serde_json::Value::Null => LIST_PRICING.to_vec(),
        serde_json::Value::Array(fields) => fields.iter().filter_map(|f| f.as_str()).collect(),
        _ => return Err(bad("fields")),
    };
    if !fields.iter().any(|f| LIST_PRICING.contains(f)) {
        return Ok(Some(none));
    }
    let previous = &event.data["previous_applies_to"];
    let parsed = |field: &str| -> Option<Vec<String>> {
        previous[field]
            .as_array()?
            .iter()
            .map(|id| id.as_str().map(str::to_string))
            .collect()
    };
    let previously = match previous["type"].as_str() {
        None if previous.is_null() => None,
        Some("all") => Some(AppliesTo::All),
        Some("products") => parsed("products")
            .and_then(|ids| ids.iter().map(|id| id.parse().ok()).collect())
            .map(AppliesTo::Products),
        Some("categories") => parsed("categories")
            .and_then(|ids| ids.iter().map(|id| id.parse().ok()).collect())
            .map(AppliesTo::Categories),
        _ => None,
    };
    if previously.is_none() && !previous.is_null() {
        return Err(bad("previous_applies_to"));
    }
    Ok(Some(ListReach {
        lists: list_ids("price_list")?,
        if_held: true,
        audience_changed: fields.contains(&"audience"),
        previously,
    }))
}

/// What `event` asks to rebuild, for the events whose payload says it all (not a price
/// list's: `marks`). `Err` when its payload isn't what its type promises.
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
