//! `TranslationCleanup`: translations have no foreign key to what they translate (one
//! table for every kind), so deletions are followed by event. Eventually consistent:
//! readers ask for translations by the resources they loaded, so a deleted resource's
//! are never shown meanwhile.

use hypha_core::{EventHandler, StoredEvent};

use crate::{
    catalog::{ResourceKind, ResourceRef},
    repo::{HasTranslations, TranslationRepo},
};

/// The deletions it follows: the event, the kind that went, and the payload's field
/// holding its ID. Shipping options and payment methods aren't here: no event names
/// one that went, so the host removes their translations where it deletes them.
pub const WANTED: [(&str, ResourceKind, &str); 6] = [
    ("product.deleted", ResourceKind::Product, "product"),
    ("category.deleted", ResourceKind::Category, "category"),
    (
        "custom_field_definition.deleted",
        ResourceKind::CustomFieldDefinition,
        "definition",
    ),
    ("discount.deleted", ResourceKind::Discount, "discount"),
    (
        "checkout_rule.deleted",
        ResourceKind::CheckoutRule,
        "checkout_rule",
    ),
    ("asset.deleted", ResourceKind::Asset, "asset"),
];

/// Removes a deleted resource's translations, in every language. Idempotent: a second
/// run finds none.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranslationCleanup;

impl<P: HasTranslations> EventHandler<P> for TranslationCleanup {
    fn name(&self) -> &'static str {
        "translation_cleanup"
    }

    fn wants(&self, event_type: &str) -> bool {
        WANTED.iter().any(|(wanted, _, _)| *wanted == event_type)
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let Some((_, kind, field)) = WANTED
            .iter()
            .find(|(wanted, _, _)| *wanted == event.event_type)
        else {
            return Ok(());
        };
        let resource = event.data[*field]
            .as_str()
            .and_then(|id| ResourceRef::parse(*kind, id).ok())
            .ok_or_else(|| format!("event {} names no {kind}", event.id))?;
        ports
            .translations()
            .delete_resources(&[resource])
            .await
            .map_err(|e| e.to_string())
    }
}
