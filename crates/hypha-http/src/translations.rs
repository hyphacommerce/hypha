//! What the translations leaf can't see: each kind's resource, read here and handed over
//! as its translatable fields in the store's default language. One function per kind
//! says which fields a resource has now and where their text is; the routes that write
//! translations and the responses that show them build their keys here.

use hypha_categories::{Category, CategoryRepo, HasCategories};
use hypha_checkout_rules::{CheckoutRule, CheckoutRuleRepo, HasCheckoutRules};
use hypha_core::{
    AssetId, CategoryId, CheckoutRuleId, CustomFieldDefinitionId, DiscountId, EventHandler,
    Language, PaymentMethodId, ProductId, RepoError, ShippingOptionId, StoreId, StoredEvent,
};
use hypha_custom_fields::{CustomFieldDefinition, DefinitionRepo, HasCustomFieldDefinitions};
use hypha_discounts::{Discount, DiscountRepo, HasDiscounts};
use hypha_media::{Asset, AssetRepo, HasAssets};
use hypha_notifications::{
    HasNotificationSettings, HasNotificationTemplates, NotificationSettings,
    NotificationSettingsRepo, NotificationTemplateRepo,
};
use hypha_payments::{HasPaymentMethods, PaymentMethod, PaymentMethodRepo};
use hypha_products::{HasProducts, Product, ProductRepo};
use hypha_search_index::{HasSearchSettings, SearchSettings, SearchSettingsRepo};
use hypha_shipping::{HasShippingZones, ShippingOption, ShippingZoneRepo};
use hypha_stores::{Store, StoreRepo};
use hypha_translations::{
    HasTranslations, ResourceKind, ResourceRef, Source, SourceField, TranslationRepo, fields,
};

use crate::{
    error::ApiError,
    state::{AppState, Storage},
};

/// A product's translatable fields: its title and description, each option's name and
/// each option value's label.
pub fn product_source(product: &Product) -> Source {
    let mut fields = vec![
        SourceField::new(fields::product::TITLE, Some(product.title())),
        SourceField::new(
            fields::product::DESCRIPTION,
            product.description().map(|d| d.as_html()),
        ),
    ];
    for option in product.options() {
        fields.push(SourceField::new(
            fields::product::option_name(option.id()),
            Some(option.name()),
        ));
    }
    for value in product.options().iter().flat_map(|o| o.values()) {
        fields.push(SourceField::new(
            fields::product::option_value_label(value.id()),
            Some(value.label()),
        ));
    }
    Source::new(fields)
}

pub fn category_source(category: &Category) -> Source {
    Source::new(vec![
        SourceField::new(fields::category::NAME, Some(category.name())),
        SourceField::new(
            fields::category::DESCRIPTION,
            category.description().map(|d| d.as_html()),
        ),
    ])
}

/// A definition's name and description, and each choice's label by the choice's key.
pub fn definition_source(definition: &CustomFieldDefinition) -> Source {
    let mut fields = vec![
        SourceField::new(
            fields::custom_field_definition::NAME,
            Some(definition.name()),
        ),
        SourceField::new(
            fields::custom_field_definition::DESCRIPTION,
            definition.description(),
        ),
    ];
    for choice in definition.validations().choices() {
        fields.push(SourceField::new(
            fields::custom_field_definition::choice_label(&choice.key),
            Some(&choice.label),
        ));
    }
    Source::new(fields)
}

pub fn shipping_option_source(option: &ShippingOption) -> Source {
    Source::new(vec![SourceField::new(
        fields::shipping_option::NAME,
        Some(option.name()),
    )])
}

pub fn payment_method_source(method: &PaymentMethod) -> Source {
    Source::new(vec![
        SourceField::new(fields::payment_method::NAME, Some(method.name())),
        SourceField::new(fields::payment_method::INSTRUCTIONS, method.instructions()),
    ])
}

pub fn discount_source(discount: &Discount) -> Source {
    Source::new(vec![SourceField::new(
        fields::discount::TITLE,
        Some(discount.title()),
    )])
}

/// What the shopper is told when the rule holds.
pub fn checkout_rule_source(rule: &CheckoutRule) -> Source {
    Source::new(vec![SourceField::new(
        fields::checkout_rule::MESSAGE,
        Some(&rule.outcome().message),
    )])
}

pub fn asset_source(asset: &Asset) -> Source {
    Source::new(vec![SourceField::new(fields::asset::ALT, asset.alt())])
}

/// The store's own text: its description, and its emails' footer from the notification
/// settings (`None` until the merchant saves them).
pub fn store_source(store: &Store, notifications: Option<&NotificationSettings>) -> Source {
    Source::new(vec![
        SourceField::new(fields::store::DESCRIPTION, store.description()),
        SourceField::new(
            fields::store::EMAIL_FOOTER,
            notifications.and_then(|n| n.footer()),
        ),
    ])
}

/// The label of each facet the merchant set up, by the facet's key (`facets.vendor.label`,
/// `facets.option.Color.label`). The default facets, shown until the merchant chooses
/// some, have no labels of their own: clients name them.
pub fn search_facet_source(settings: Option<&SearchSettings>) -> Source {
    let facets = settings.map(|s| s.facets()).unwrap_or_default();
    Source::new(
        facets
            .iter()
            .map(|facet| {
                SourceField::new(
                    fields::search_facet::facet_label(&facet.key),
                    facet.label.as_deref(),
                )
            })
            .collect(),
    )
}

/// `resource` as it is now, `None` when there's no such resource. Reads through the
/// repositories, not the domains' commands: the caller checked the scopes
/// (`hypha_translations::require_read`) before asking.
pub(crate) async fn source<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
    resource: ResourceRef,
) -> Result<Option<Source>, ApiError> {
    let id = resource.id;
    Ok(match resource.kind {
        ResourceKind::Product => ports
            .products()
            .get(ProductId::from_uuid(id))
            .await?
            .as_ref()
            .map(product_source),
        ResourceKind::Category => ports
            .categories()
            .get(CategoryId::from_uuid(id))
            .await?
            .as_ref()
            .map(category_source),
        ResourceKind::CustomFieldDefinition => ports
            .custom_field_definitions()
            .get(CustomFieldDefinitionId::from_uuid(id))
            .await?
            .as_ref()
            .map(definition_source),
        ResourceKind::ShippingOption => {
            // Options are their zones' children, and a store has a handful of zones.
            let option = ShippingOptionId::from_uuid(id);
            let zones = ports.shipping_zones().list().await?;
            zones
                .iter()
                .flat_map(|zone| zone.options())
                .find(|o| o.id() == option)
                .map(shipping_option_source)
        }
        ResourceKind::PaymentMethod => ports
            .payment_methods()
            .get(PaymentMethodId::from_uuid(id))
            .await?
            .as_ref()
            .map(payment_method_source),
        ResourceKind::Discount => ports
            .discounts()
            .get(DiscountId::from_uuid(id))
            .await?
            .as_ref()
            .map(discount_source),
        ResourceKind::CheckoutRule => ports
            .checkout_rules()
            .get(CheckoutRuleId::from_uuid(id))
            .await?
            .as_ref()
            .map(checkout_rule_source),
        ResourceKind::Asset => ports
            .assets()
            .get(AssetId::from_uuid(id))
            .await?
            .as_ref()
            .map(asset_source),
        // One per store, named by the store's ID.
        ResourceKind::Store if id == state.store.as_uuid() => {
            let notifications = ports.notification_settings().get().await?;
            let store = crate::routes::stores::store_as_system(state, ports).await?;
            Some(store_source(&store, notifications.as_ref()))
        }
        ResourceKind::SearchFacet if id == state.store.as_uuid() => {
            let settings = ports.search_settings().get().await?;
            Some(search_facet_source(settings.as_ref()))
        }
        ResourceKind::Store | ResourceKind::SearchFacet => None,
    })
}

/// Removes the translations of resources that are gone, for the kinds no event names
/// when they go (shipping options, payment methods): the route that deleted them calls
/// this with the same ports, after the delete committed.
///
/// Best effort: the delete stands, so its answer doesn't change when this fails; the
/// failure is logged and the translations stay behind, unread (nothing lists a gone
/// resource's). A deleted event for these kinds, followed by a handler like other
/// kinds', would retry it; it's owed.
pub(crate) async fn forget<P: HasTranslations>(
    ports: &P,
    kind: ResourceKind,
    ids: impl IntoIterator<Item = uuid::Uuid>,
) {
    let resources: Vec<ResourceRef> = ids
        .into_iter()
        .map(|id| ResourceRef::new(kind, id))
        .collect();
    if let Err(error) = ports.translations().delete_resources(&resources).await {
        let ids: Vec<String> = resources.iter().map(ResourceRef::id_string).collect();
        tracing::warn!(
            %error,
            %kind,
            ?ids,
            "a deleted resource's translations weren't removed"
        );
    }
}

/// Removes the translations into languages the store no longer lists, and the email
/// templates the merchant saved for them. A language is
/// taken off the list by a write of the store, which may be on another connection than
/// the translations, so it's followed by event: `store.updated` naming `languages`.
/// Idempotent: it keeps what the list holds when it runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct LanguageCleanup;

impl<P: StoreRepo + HasTranslations + HasNotificationTemplates> EventHandler<P>
    for LanguageCleanup
{
    fn name(&self) -> &'static str {
        "language_cleanup"
    }

    fn wants(&self, event_type: &str) -> bool {
        event_type == "store.updated"
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let names_languages = event.data["fields"]
            .as_array()
            .is_some_and(|fields| fields.iter().any(|f| f == "languages"));
        if !names_languages {
            return Ok(());
        }
        let id: StoreId = event.data["store"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| format!("event {} names no store", event.id))?;
        let store = StoreRepo::get(ports, id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("store {id} not found"))?;
        let listed: Vec<Language> = store
            .languages()
            .iter()
            .map(|l| l.language.clone())
            .collect();
        ports
            .translations()
            .retain_languages(&listed)
            .await
            .map_err(|e| e.to_string())?;
        let templates = ports.notification_templates();
        let stored = templates.list().await.map_err(|e| e.to_string())?;
        for template in stored
            .iter()
            .filter(|t| t.locale().is_some_and(|l| !listed.contains(l)))
        {
            match templates.delete(template).await {
                // Changed or removed since it was read: another run's, or a save that
                // a later run of this handler sees.
                Ok(()) | Err(RepoError::StaleVersion) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_translations::FieldRule;

    use super::*;

    fn limit(kind: ResourceKind, key: &str) -> usize {
        match kind.field(key).map(|(spec, _)| spec.rule) {
            Some(FieldRule::Text { max_chars }) => max_chars,
            Some(FieldRule::Html { max_bytes }) => max_bytes,
            None => 0,
        }
    }

    /// A translation obeys the rule of the field it replaces: the catalog's limits are
    /// the domains' own.
    #[test]
    fn the_catalogs_limits_are_the_domains() {
        use ResourceKind as K;
        let text = hypha_core::MAX_TEXT_LEN;
        let html = hypha_core::Description::MAX_LEN;
        let expected = [
            (K::Product, "title", text),
            (K::Product, "description", html),
            (K::Product, "options.opt_1.name", text),
            (K::Product, "option_values.optval_1.label", text),
            (K::Category, "name", text),
            (K::Category, "description", html),
            (
                K::CustomFieldDefinition,
                "name",
                hypha_custom_fields::limits::MAX_NAME_CHARS,
            ),
            (
                K::CustomFieldDefinition,
                "description",
                hypha_custom_fields::limits::MAX_DESCRIPTION_CHARS,
            ),
            (K::CustomFieldDefinition, "choices.red.label", text),
            (K::ShippingOption, "name", text),
            (K::PaymentMethod, "name", text),
            (
                K::PaymentMethod,
                "instructions",
                PaymentMethod::MAX_INSTRUCTIONS_LEN,
            ),
            (K::Discount, "title", text),
            (
                K::CheckoutRule,
                "message",
                hypha_checkout_rules::MAX_MESSAGE_CHARS,
            ),
            (K::Asset, "alt", text),
            (K::Store, "description", Store::MAX_DESCRIPTION_CHARS),
            (
                K::Store,
                "email_footer",
                NotificationSettings::MAX_FOOTER_LEN,
            ),
            (K::SearchFacet, "facets.vendor.label", text),
        ];
        for (kind, key, max) in expected {
            assert_eq!(limit(kind, key), max, "{kind} {key}");
        }
        // Every field of the catalog is held to something above.
        let fields: usize = K::ALL.iter().map(|k| k.fields().len()).sum();
        assert_eq!(fields, expected.len());
    }
}
