//! What happens to products, as events (design note 0016, section 4). Contract types: a
//! field renamed or reshaped is a new `VERSION`, never an edit.
//!
//! Payloads carry the subject and what changed, not the product: a consumer that needs
//! it fetches it (Q2).

use std::collections::{BTreeMap, BTreeSet};

use hypha_core::{Event, PendingEvent, ProductId, Subject, VariantId};
use serde::Serialize;

/// The subject kind events about products carry.
pub const PRODUCT: &str = "product";

/// `product.created`.
#[derive(Debug, Serialize)]
pub struct ProductCreated {
    pub product: ProductId,
    pub handle: String,
    pub status: &'static str,
}

impl Event for ProductCreated {
    const TYPE: &'static str = "product.created";
    const VERSION: u16 = 1;
}

/// `product.updated`: which of the product's own fields changed, among `title`,
/// `handle`, `status`, `description`, `vendor`, `product_type`, `tags`, `options` and
/// `media`.
#[derive(Debug, Serialize)]
pub struct ProductUpdated {
    pub product: ProductId,
    pub fields: Vec<&'static str>,
}

impl Event for ProductUpdated {
    const TYPE: &'static str = "product.updated";
    const VERSION: u16 = 1;
}

/// `product.variant_added`.
#[derive(Debug, Serialize)]
pub struct VariantAdded {
    pub product: ProductId,
    pub variant: VariantId,
}

impl Event for VariantAdded {
    const TYPE: &'static str = "product.variant_added";
    const VERSION: u16 = 1;
}

/// `product.variant_updated`: which of the variant's fields changed, among `sku`,
/// `price`, `selections`, `weight`, `requires_shipping` and `media`.
#[derive(Debug, Serialize)]
pub struct VariantUpdated {
    pub product: ProductId,
    pub variant: VariantId,
    pub fields: Vec<&'static str>,
}

impl Event for VariantUpdated {
    const TYPE: &'static str = "product.variant_updated";
    const VERSION: u16 = 1;
}

/// `product.variant_removed`: other domains clean up what they keep for the variant
/// (fixed prices, its inventory item; design note 0014, section 4).
#[derive(Debug, Serialize)]
pub struct VariantRemoved {
    pub product: ProductId,
    pub variant: VariantId,
}

impl Event for VariantRemoved {
    const TYPE: &'static str = "product.variant_removed";
    const VERSION: u16 = 1;
}

/// What changed in a product since it was loaded, coalesced: one `product.updated` per
/// write listing every field, one `product.variant_updated` per variant. Compares as
/// always equal, so it never makes two products unequal.
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    created: bool,
    fields: BTreeSet<&'static str>,
    added: Vec<VariantId>,
    removed: Vec<VariantId>,
    updated: BTreeMap<VariantId, BTreeSet<&'static str>>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Changes {
    /// A new product: its creation is the whole story, whatever else was set up.
    pub(crate) fn created() -> Self {
        Self {
            created: true,
            ..Self::default()
        }
    }

    pub(crate) fn field(&mut self, field: &'static str) {
        self.fields.insert(field);
    }

    pub(crate) fn variant_added(&mut self, variant: VariantId) {
        self.added.push(variant);
    }

    pub(crate) fn variant_removed(&mut self, variant: VariantId) {
        match self.added.iter().position(|v| *v == variant) {
            // Added and removed in one write: nothing happened to it, as far as anyone
            // outside can tell.
            Some(i) => {
                self.added.remove(i);
            }
            None => self.removed.push(variant),
        }
        self.updated.remove(&variant);
    }

    pub(crate) fn variant_field(&mut self, variant: VariantId, field: &'static str) {
        if !self.added.contains(&variant) {
            self.updated.entry(variant).or_default().insert(field);
        }
    }

    /// The events, and no changes left.
    pub(crate) fn take(
        &mut self,
        product: ProductId,
        handle: &str,
        status: &'static str,
    ) -> Vec<PendingEvent> {
        let changes = std::mem::take(self);
        let subject = Subject::new(PRODUCT, product.as_uuid());
        if changes.created {
            let created = ProductCreated {
                product,
                handle: handle.to_string(),
                status,
            };
            return vec![PendingEvent::new(subject, &created)];
        }
        let mut events = Vec::new();
        if !changes.fields.is_empty() {
            let fields = changes.fields.into_iter().collect();
            events.push(PendingEvent::new(
                subject,
                &ProductUpdated { product, fields },
            ));
        }
        for variant in changes.added {
            events.push(PendingEvent::new(
                subject,
                &VariantAdded { product, variant },
            ));
        }
        for (variant, fields) in changes.updated {
            let fields = fields.into_iter().collect();
            let updated = VariantUpdated {
                product,
                variant,
                fields,
            };
            events.push(PendingEvent::new(subject, &updated));
        }
        for variant in changes.removed {
            events.push(PendingEvent::new(
                subject,
                &VariantRemoved { product, variant },
            ));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn product() -> ProductId {
        ProductId::from_uuid(Uuid::from_u128(1))
    }
    fn variant(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn types(events: &[PendingEvent]) -> Vec<&str> {
        events.iter().map(|e| e.event_type).collect()
    }

    fn take(changes: &mut Changes) -> Vec<PendingEvent> {
        changes.take(product(), "mug", "draft")
    }

    #[test]
    fn a_new_product_is_only_created() {
        let mut changes = Changes::created();
        changes.field("title");
        changes.variant_added(variant(2));
        let events = take(&mut changes);
        assert_eq!(types(&events), ["product.created"]);
        assert_eq!(events[0].data["status"], "draft");
    }

    #[test]
    fn field_changes_are_one_update_listing_each_once() {
        let mut changes = Changes::default();
        changes.field("title");
        changes.field("handle");
        changes.field("title");
        let events = take(&mut changes);
        assert_eq!(types(&events), ["product.updated"]);
        assert_eq!(
            events[0].data["fields"],
            serde_json::json!(["handle", "title"])
        );
    }

    #[test]
    fn a_variant_added_and_removed_in_one_write_leaves_no_trace() {
        let mut changes = Changes::default();
        changes.variant_added(variant(2));
        changes.variant_field(variant(2), "price");
        changes.variant_removed(variant(2));
        assert!(take(&mut changes).is_empty());
    }

    #[test]
    fn a_removed_variant_drops_its_updates() {
        let mut changes = Changes::default();
        changes.variant_field(variant(2), "price");
        changes.variant_removed(variant(2));
        let events = take(&mut changes);
        assert_eq!(types(&events), ["product.variant_removed"]);
        assert_eq!(events[0].data["variant"], variant(2).to_string());
    }

    #[test]
    fn a_new_variant_is_added_not_updated() {
        let mut changes = Changes::default();
        changes.variant_added(variant(2));
        changes.variant_field(variant(2), "media");
        assert_eq!(types(&take(&mut changes)), ["product.variant_added"]);
    }

    #[test]
    fn taking_leaves_nothing() {
        let mut changes = Changes::default();
        changes.field("title");
        take(&mut changes);
        assert!(take(&mut changes).is_empty());
    }
}
