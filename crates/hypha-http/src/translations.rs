//! What the translations leaf can't see: each kind's resource, read here and handed over
//! as its translatable fields in the store's default language. One function per kind
//! says which fields a resource has now and where their text is; the routes that write
//! translations and the responses that show them build their keys here.

use hypha_categories::{Category, CategoryRepo, HasCategories};
use hypha_checkout_rules::{CheckoutRule, CheckoutRuleRepo, HasCheckoutRules};
use std::collections::BTreeMap;

use hypha_core::{
    AssetId, CategoryId, CheckoutRuleId, CustomFieldDefinitionId, DiscountId, EventHandler,
    Language, PaymentMethodId, ProductId, RepoError, ShippingOptionId, ShippingZoneId, StoreId,
    StoredEvent, VariantId,
};
use hypha_custom_fields::{
    AdminAccess, BaseType, CustomFieldDefinition, DefinitionRepo, FieldKey, FieldsRead,
    HasCustomFieldDefinitions, HasCustomFieldValues, Namespace, OwnerType, StorefrontAccess,
    ValueRepo,
    limits::{MAX_LONG_BYTES, MAX_TEXT_CHARS},
};
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
    FieldRule, HasTranslations, ResourceKind, ResourceRef, Source, SourceField, TranslationRepo,
    TranslationSourceRepo, ValueFormat, ValueRule, fields, source_digest, sync_sources,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    error::ApiError,
    state::{AppState, Storage},
};

/// A resource as the translation routes read it: its fields now, and what names each
/// custom field value among them (by its key), which a key alone doesn't.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    pub source: Source,
    pub values: BTreeMap<String, TranslatedValue>,
}

impl From<Source> for Loaded {
    fn from(source: Source) -> Self {
        Self {
            source,
            values: BTreeMap::new(),
        }
    }
}

/// The custom field value a translatable field is, for an editor to label it: its
/// definition, and the variant whose value it is (in a product's translation).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct TranslatedValue {
    pub definition: CustomFieldDefinitionId,
    /// The field's key, `custom.material`.
    pub key: String,
    /// The definition's name, in the store's default language.
    pub name: String,
    /// `text`, `multiline_text` or `rich_text`.
    #[serde(rename = "type")]
    pub field_type: &'static str,
    /// The variant whose value it is, in a product's translation; `null` for the
    /// resource's own.
    pub variant: Option<VariantId>,
    /// That variant's title as stored ("M / Blue"); `null` for the resource's own value
    /// and a variant without options.
    pub variant_title: Option<String>,
}

/// Whether `definition`'s values are translated: prose a shopper reads. A single
/// `text`, `multiline_text` or `rich_text` field of a product, variant or category, that
/// storefronts read (`access.storefront: read`), not an app's private one, and without a
/// `pattern` (its values are codes), whose values the merchant writes (their own, or an
/// app's or spore's with `access.admin: read_write`: translating one an app keeps
/// read-only would change what shoppers see of it). Customers' and orders' fields hold personal data and
/// are never translated; lists are filter values, like tags. A definition that stops
/// qualifying keeps its translations stored until the next write of each, and isn't read
/// meanwhile.
pub fn translatable_value(definition: &CustomFieldDefinition) -> bool {
    let owner = matches!(
        definition.owner_type(),
        OwnerType::Product | OwnerType::Variant | OwnerType::Category
    );
    let field_type = definition.field_type();
    let text = !field_type.is_list()
        && matches!(
            field_type.base(),
            BaseType::Text | BaseType::MultilineText | BaseType::RichText
        );
    owner
        && text
        && read_by_shoppers(definition)
        && definition.validations().pattern().is_none()
        && (*definition.namespace() == Namespace::Custom
            || definition.access().admin == AdminAccess::ReadWrite)
}

/// What a translation of `definition`'s value is held to: what a value is (one line for
/// `text`, the type's byte cap, rich text sanitised), within the definition's lengths.
pub fn value_rule(definition: &CustomFieldDefinition) -> FieldRule {
    let validations = definition.validations();
    let chars = |n: Option<u32>| n.and_then(|n| usize::try_from(n).ok());
    let (min, max) = (
        chars(validations.min_length()),
        chars(validations.max_length()),
    );
    let rule = match definition.field_type().base() {
        BaseType::MultilineText => ValueRule {
            format: ValueFormat::Lines,
            min_chars: min,
            max_chars: max,
            max_bytes: Some(MAX_LONG_BYTES),
        },
        BaseType::RichText => ValueRule {
            format: ValueFormat::Html,
            min_chars: None,
            max_chars: max,
            max_bytes: Some(MAX_LONG_BYTES),
        },
        _ => ValueRule {
            format: ValueFormat::Line,
            min_chars: min,
            max_chars: Some(max.map_or(MAX_TEXT_CHARS, |m| m.min(MAX_TEXT_CHARS))),
            max_bytes: None,
        },
    };
    FieldRule::Value(rule)
}

/// A translatable custom field value: its owner (the resource, or one of a product's
/// variants), its definition and its text as stored.
#[derive(Debug, Clone)]
pub struct ValueText {
    pub owner: Uuid,
    pub definition: CustomFieldDefinition,
    pub text: String,
}

/// The translatable values (`translatable_value`) `owners` hold, each owner of its type,
/// in one read of definitions and one of values; by owner as given, then by the
/// definitions' positions.
pub async fn value_texts<P: HasCustomFieldDefinitions + HasCustomFieldValues>(
    ports: &P,
    owners: &[(Uuid, OwnerType)],
) -> Result<Vec<ValueText>, RepoError> {
    // A category's are of one type; a product's of two, read together.
    let only = match owners {
        [(_, owner_type)] => Some(*owner_type),
        _ => None,
    };
    let definitions: Vec<CustomFieldDefinition> = ports
        .custom_field_definitions()
        .list(only)
        .await?
        .into_iter()
        .filter(|d| translatable_value(d) && owners.iter().any(|(_, t)| *t == d.owner_type()))
        .collect();
    if definitions.is_empty() || owners.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<CustomFieldDefinitionId> = definitions.iter().map(|d| d.id()).collect();
    let uuids: Vec<Uuid> = owners.iter().map(|(id, _)| *id).collect();
    let stored = ports
        .custom_field_values()
        .get_many(&uuids, Some(&ids))
        .await?;
    let mut texts = Vec::new();
    for (owner, owner_type) in owners {
        let mut of_owner: Vec<ValueText> = stored
            .iter()
            .filter(|row| row.owner == *owner)
            .filter_map(|row| {
                let definition = definitions
                    .iter()
                    .find(|d| d.id() == row.definition && d.owner_type() == *owner_type)?;
                // Text, multiline and rich text values are stored as a JSON string.
                let text = row.value.as_str().filter(|t| !t.is_empty())?;
                Some(ValueText {
                    owner: *owner,
                    definition: definition.clone(),
                    text: text.to_string(),
                })
            })
            .collect();
        of_owner.sort_by_key(|v| (v.definition.position(), v.definition.id()));
        texts.extend(of_owner);
    }
    Ok(texts)
}

/// `values`' fields under the keys `key` gives each, with their labels.
fn value_fields(
    values: &[ValueText],
    key: impl Fn(&ValueText) -> (String, Option<VariantId>, Option<String>),
    fields: &mut Vec<SourceField>,
    labels: &mut BTreeMap<String, TranslatedValue>,
) {
    for value in values {
        let (field_key, variant, variant_title) = key(value);
        let d = &value.definition;
        fields
            .push(SourceField::new(field_key.clone(), Some(&value.text)).with_rule(value_rule(d)));
        labels.insert(
            field_key,
            TranslatedValue {
                definition: d.id(),
                key: d.field_key().to_string(),
                name: d.name().to_string(),
                field_type: d.field_type().base().as_str(),
                variant,
                variant_title,
            },
        );
    }
}

/// `owner`'s values as a storefront reads them (`FieldsRead::json_of`), each translatable
/// one as `text` translates it (by its definition), else as stored.
pub fn translated_values<'t>(
    fields: &FieldsRead,
    owner: Uuid,
    text: impl Fn(CustomFieldDefinitionId) -> Option<&'t str>,
) -> Map<String, Value> {
    fields.json_with(owner, |entry| {
        let translated = translatable_value(&entry.definition)
            .then(|| text(entry.definition.id()))
            .flatten()?;
        Some(Value::String(translated.to_string()))
    })
}

/// A product's translatable fields: its title and description, each option's name and
/// each option value's label, then its own custom field values and each variant's
/// (`values`, from `value_texts`).
pub fn product_source(product: &Product, values: &[ValueText]) -> Loaded {
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
    let mut labels = BTreeMap::new();
    let own = product.id().as_uuid();
    value_fields(
        values,
        |v| match v.owner == own {
            true => (fields::custom_field::value(v.definition.id()), None, None),
            false => {
                let variant = VariantId::from_uuid(v.owner);
                (
                    fields::custom_field::variant_value(variant, v.definition.id()),
                    Some(variant),
                    product.variant_title(variant),
                )
            }
        },
        &mut fields,
        &mut labels,
    );
    Loaded {
        source: Source::new(fields),
        values: labels,
    }
}

/// The owners a product's custom field values are read from: itself and its variants.
pub fn product_owners(product: &Product) -> Vec<(Uuid, OwnerType)> {
    std::iter::once((product.id().as_uuid(), OwnerType::Product))
        .chain(
            product
                .variants()
                .iter()
                .map(|v| (v.id().as_uuid(), OwnerType::Variant)),
        )
        .collect()
}

/// A category's name and description, then its custom field values (`values`).
pub fn category_source(category: &Category, values: &[ValueText]) -> Loaded {
    let mut fields = vec![
        SourceField::new(fields::category::NAME, Some(category.name())),
        SourceField::new(
            fields::category::DESCRIPTION,
            category.description().map(|d| d.as_html()),
        ),
    ];
    let mut labels = BTreeMap::new();
    value_fields(
        values,
        |v| (fields::custom_field::value(v.definition.id()), None, None),
        &mut fields,
        &mut labels,
    );
    Loaded {
        source: Source::new(fields),
        values: labels,
    }
}

/// Whether shoppers ever read `definition`: storefronts see it (`access.storefront:
/// read`) and it isn't an app's private one. Only then is anything of it translated; staff
/// read its source text in the admin.
pub fn read_by_shoppers(definition: &CustomFieldDefinition) -> bool {
    definition.access().storefront == StorefrontAccess::Read && !definition.private()
}

/// A definition's name and description, and each choice's label by the choice's key,
/// while shoppers read it (`read_by_shoppers`); none otherwise, so a staff-only field has
/// nothing to translate and its stored translations go with the next write.
pub fn definition_source(definition: &CustomFieldDefinition) -> Source {
    if !read_by_shoppers(definition) {
        return Source::new(Vec::new());
    }
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

/// What reading a resource's source takes: every kind's repositories.
pub trait SourcePorts:
    HasProducts
    + HasCategories
    + HasCustomFieldDefinitions
    + HasCustomFieldValues
    + HasShippingZones
    + HasPaymentMethods
    + HasDiscounts
    + HasCheckoutRules
    + HasAssets
    + HasNotificationSettings
    + HasSearchSettings
{
}

impl<T> SourcePorts for T where
    T: HasProducts
        + HasCategories
        + HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasShippingZones
        + HasPaymentMethods
        + HasDiscounts
        + HasCheckoutRules
        + HasAssets
        + HasNotificationSettings
        + HasSearchSettings
{
}

/// `resource` as it is now, `None` when there's no such resource: the one reading of each
/// kind, which the translation routes, the worklist's handler and its job all use, so the
/// digests they keep are the editor's. Reads through the repositories, not the domains'
/// commands: callers checked the scopes (`hypha_translations::require_read`) or are the
/// system. `stores` holds the store (`store`), whose kinds are one per store.
pub async fn load_source<P: SourcePorts>(
    ports: &P,
    stores: &impl StoreRepo,
    store: StoreId,
    resource: ResourceRef,
) -> Result<Option<Loaded>, RepoError> {
    let id = resource.id;
    Ok(match resource.kind {
        ResourceKind::Product => match ports.products().get(ProductId::from_uuid(id)).await? {
            Some(product) => {
                let values = value_texts(ports, &product_owners(&product)).await?;
                Some(product_source(&product, &values))
            }
            None => None,
        },
        ResourceKind::Category => match ports.categories().get(CategoryId::from_uuid(id)).await? {
            Some(category) => {
                let values = value_texts(ports, &[(id, OwnerType::Category)]).await?;
                Some(category_source(&category, &values))
            }
            None => None,
        },
        ResourceKind::CustomFieldDefinition => ports
            .custom_field_definitions()
            .get(CustomFieldDefinitionId::from_uuid(id))
            .await?
            .as_ref()
            .map(|d| definition_source(d).into()),
        ResourceKind::ShippingOption => {
            // Options are their zones' children, and a store has a handful of zones.
            let option = ShippingOptionId::from_uuid(id);
            let zones = ports.shipping_zones().list().await?;
            zones
                .iter()
                .flat_map(|zone| zone.options())
                .find(|o| o.id() == option)
                .map(|o| shipping_option_source(o).into())
        }
        ResourceKind::PaymentMethod => ports
            .payment_methods()
            .get(PaymentMethodId::from_uuid(id))
            .await?
            .as_ref()
            .map(|m| payment_method_source(m).into()),
        ResourceKind::Discount => ports
            .discounts()
            .get(DiscountId::from_uuid(id))
            .await?
            .as_ref()
            .map(|d| discount_source(d).into()),
        ResourceKind::CheckoutRule => ports
            .checkout_rules()
            .get(CheckoutRuleId::from_uuid(id))
            .await?
            .as_ref()
            .map(|r| checkout_rule_source(r).into()),
        ResourceKind::Asset => ports
            .assets()
            .get(AssetId::from_uuid(id))
            .await?
            .as_ref()
            .map(|a| asset_source(a).into()),
        // One per store, named by the store's ID.
        ResourceKind::Store if id == store.as_uuid() => {
            match StoreRepo::get(stores, store).await? {
                Some(found) => {
                    let notifications = ports.notification_settings().get().await?;
                    Some(store_source(&found, notifications.as_ref()).into())
                }
                None => None,
            }
        }
        ResourceKind::SearchFacet if id == store.as_uuid() => {
            let settings = ports.search_settings().get().await?;
            Some(search_facet_source(settings.as_ref()).into())
        }
        ResourceKind::Store | ResourceKind::SearchFacet => None,
    })
}

/// `load_source` for a request's (or a job's) ports, with the store read through the
/// host's directory.
pub(crate) async fn source<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
    resource: ResourceRef,
) -> Result<Option<Loaded>, ApiError> {
    Ok(match state.storage.directory_in(ports) {
        Some(directory) => load_source(ports, directory, state.store, resource).await?,
        None => {
            let directory = state.storage.directory().await?;
            load_source(ports, &directory, state.store, resource).await?
        }
    })
}

/// Reads `resource` again and writes its digests for the worklist (none when it's gone).
pub async fn sync_source<P: SourcePorts + HasTranslations>(
    ports: &P,
    stores: &impl StoreRepo,
    store: StoreId,
    resource: ResourceRef,
) -> Result<(), RepoError> {
    let loaded = load_source(ports, stores, store, resource).await?;
    sync_sources(ports, resource, loaded.as_ref().map(|l| &l.source)).await
}

/// The events about one resource's translatable text: its type, the kind, and the
/// payload's field holding its ID. A deletion is among them: the resource read again is
/// gone, and its digests go.
const RESOURCE_EVENTS: [(&str, ResourceKind, &str); 23] = [
    ("product.created", ResourceKind::Product, "product"),
    ("product.updated", ResourceKind::Product, "product"),
    ("product.variant_added", ResourceKind::Product, "product"),
    ("product.variant_updated", ResourceKind::Product, "product"),
    ("product.variant_removed", ResourceKind::Product, "product"),
    (
        "product.custom_fields_updated",
        ResourceKind::Product,
        "product",
    ),
    ("product.deleted", ResourceKind::Product, "product"),
    ("category.changed", ResourceKind::Category, "category"),
    (
        "category.custom_fields_updated",
        ResourceKind::Category,
        "category",
    ),
    ("category.deleted", ResourceKind::Category, "category"),
    (
        "custom_field_definition.created",
        ResourceKind::CustomFieldDefinition,
        "definition",
    ),
    (
        "custom_field_definition.updated",
        ResourceKind::CustomFieldDefinition,
        "definition",
    ),
    (
        "custom_field_definition.deleted",
        ResourceKind::CustomFieldDefinition,
        "definition",
    ),
    (
        "shipping_option.deleted",
        ResourceKind::ShippingOption,
        "shipping_option",
    ),
    (
        "payment_method.created",
        ResourceKind::PaymentMethod,
        "payment_method",
    ),
    (
        "payment_method.updated",
        ResourceKind::PaymentMethod,
        "payment_method",
    ),
    (
        "payment_method.deleted",
        ResourceKind::PaymentMethod,
        "payment_method",
    ),
    ("discount.created", ResourceKind::Discount, "discount"),
    ("discount.updated", ResourceKind::Discount, "discount"),
    ("discount.deleted", ResourceKind::Discount, "discount"),
    (
        "checkout_rule.created",
        ResourceKind::CheckoutRule,
        "checkout_rule",
    ),
    (
        "checkout_rule.updated",
        ResourceKind::CheckoutRule,
        "checkout_rule",
    ),
    (
        "checkout_rule.deleted",
        ResourceKind::CheckoutRule,
        "checkout_rule",
    ),
];

/// The other events it follows: assets' (whose alt is their text), a zone's (its options
/// are its children), and the store's kinds, which are one per store.
const ASSET_UPDATED: &str = "asset.updated";
const ASSET_DELETED: &str = "asset.deleted";
const ZONE_CHANGED: &str = "shipping_zone.changed";
const STORE_UPDATED: &str = "store.updated";
const NOTIFICATION_SETTINGS_UPDATED: &str = "notification_settings.updated";
const SEARCH_SETTINGS_CHANGED: &str = "search_settings.changed";
const OTHER_EVENTS: [&str; 6] = [
    ASSET_UPDATED,
    ASSET_DELETED,
    ZONE_CHANGED,
    STORE_UPDATED,
    NOTIFICATION_SETTINGS_UPDATED,
    SEARCH_SETTINGS_CHANGED,
];

/// What about a custom field definition decides whether its values are translated
/// (`translatable_value`): a change to one reaches every value it has.
const TRANSLATABILITY: [&str; 3] = ["access", "private", "validations"];

/// Keeps the worklist's digests (`translation_sources`) in step with the resources: on
/// every event that may change a resource's translatable text, it reads that resource
/// again (`sync_source`, the same reading as the translation editor's) and writes its
/// digests, or removes them when it's gone. Here rather than in the leaf, which can't see
/// the resources. Idempotent and order-blind: what's written is the resource as it is
/// when the delivery runs, so a retry or a late delivery writes the same. A custom field
/// definition whose values' translatability may have changed (deleted, or its access,
/// privacy or validations changed) queues every product or category for the
/// `translation_sources` job, which reads them again in batches.
#[derive(Debug, Clone, Copy)]
pub struct TranslationSourceSync {
    store: StoreId,
}

impl TranslationSourceSync {
    /// For the store `store`: the resource of its one-per-store kinds.
    pub fn new(store: StoreId) -> Self {
        Self { store }
    }

    /// The resources `event` is about, and the kind it reaches as a whole, if any.
    async fn targets<P: SourcePorts>(
        &self,
        ports: &P,
        event: &StoredEvent,
    ) -> Result<Targets, String> {
        let named = |kind: ResourceKind, field: &str| {
            event.data[field]
                .as_str()
                .and_then(|id| ResourceRef::parse(kind, id).ok())
                .ok_or_else(|| format!("event {} names no {kind}", event.id))
        };
        let store = |kind| ResourceRef::new(kind, self.store.as_uuid());
        let event_type = event.event_type.as_str();
        if let Some((_, kind, field)) = RESOURCE_EVENTS.iter().find(|(t, _, _)| *t == event_type) {
            let resource = named(*kind, field)?;
            let whole = (*kind == ResourceKind::CustomFieldDefinition)
                .then(|| owners_reached(event))
                .flatten();
            return Ok(Targets {
                reread: vec![resource],
                whole,
                ..Targets::default()
            });
        }
        let reread = |resources| Targets {
            reread: resources,
            ..Targets::default()
        };
        Ok(match event_type {
            ASSET_UPDATED | ASSET_DELETED => reread(vec![named(ResourceKind::Asset, "asset")?]),
            ZONE_CHANGED => {
                let zone: ShippingZoneId = event.data["shipping_zone"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| format!("event {} names no shipping zone", event.id))?;
                // The zone holds its options: read once, not once per option. A zone
                // deleted since has its options' own deletions.
                let known = ports
                    .shipping_zones()
                    .get(zone)
                    .await
                    .map_err(|e| e.to_string())?
                    .map(|zone| {
                        zone.options()
                            .iter()
                            .map(|o| {
                                (
                                    ResourceRef::new(
                                        ResourceKind::ShippingOption,
                                        o.id().as_uuid(),
                                    ),
                                    shipping_option_source(o),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Targets {
                    known,
                    ..Targets::default()
                }
            }
            STORE_UPDATED | NOTIFICATION_SETTINGS_UPDATED => {
                reread(vec![store(ResourceKind::Store)])
            }
            SEARCH_SETTINGS_CHANGED => reread(vec![store(ResourceKind::SearchFacet)]),
            _ => Targets::default(),
        })
    }
}

/// What an event has the handler write: resources to read again, resources already read
/// (a zone's options, with their zone), and a kind to queue whole.
#[derive(Default)]
struct Targets {
    reread: Vec<ResourceRef>,
    known: Vec<(ResourceRef, Source)>,
    whole: Option<ResourceKind>,
}

/// The kind whose every resource a definition's event reaches: a product's, variant's or
/// category's definition deleted, or changed in what makes its values translatable.
fn owners_reached(event: &StoredEvent) -> Option<ResourceKind> {
    let reaches = match event.event_type.as_str() {
        "custom_field_definition.deleted" => true,
        "custom_field_definition.updated" => {
            event.data["fields"].as_array().is_some_and(|fields| {
                fields
                    .iter()
                    .any(|f| TRANSLATABILITY.iter().any(|t| f == t))
            })
        }
        _ => false,
    };
    if !reaches {
        return None;
    }
    match event.data["owner_type"].as_str()? {
        "product" | "variant" => Some(ResourceKind::Product),
        "category" => Some(ResourceKind::Category),
        _ => None,
    }
}

impl<P: SourcePorts + StoreRepo + HasTranslations> EventHandler<P> for TranslationSourceSync {
    fn name(&self) -> &'static str {
        "translation_source_sync"
    }

    fn wants(&self, event_type: &str) -> bool {
        RESOURCE_EVENTS.iter().any(|(t, _, _)| *t == event_type)
            || OTHER_EVENTS.contains(&event_type)
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let fail = |e: RepoError| e.to_string();
        let targets = self.targets(ports, event).await?;
        let mut written = Vec::new();
        for resource in targets.reread {
            sync_source(ports, ports, self.store, resource)
                .await
                .map_err(fail)?;
            written.push(resource);
        }
        for (resource, source) in &targets.known {
            sync_sources(ports, *resource, Some(source))
                .await
                .map_err(fail)?;
            written.push(*resource);
        }
        // The job may be reading one of these from an older copy: queued, it reads it
        // again rather than unqueue what it wrote over this.
        ports
            .translations()
            .touch_queued(&written)
            .await
            .map_err(fail)?;
        if let Some(kind) = targets.whole {
            ports
                .translations()
                .queue_kind(kind)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
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

/// Removes the translation of a custom field value that was removed, in every language,
/// so a value set again later doesn't come back with the old one's translation: a field
/// is kept only when it translates the value there now (its digest is that value's). A
/// removed owner's go with its translations (`TranslationCleanup`), a deleted
/// definition's and a removed variant's stay until the next write and are never read
/// (their IDs aren't used again). Follows `<owner>.custom_fields_updated` naming a value
/// removed (`"value": null`). Idempotent.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValueTranslationCleanup;

const PRODUCT_FIELDS_UPDATED: &str = "product.custom_fields_updated";
const CATEGORY_FIELDS_UPDATED: &str = "category.custom_fields_updated";

impl<P: HasTranslations + HasCustomFieldDefinitions + HasCustomFieldValues> EventHandler<P>
    for ValueTranslationCleanup
{
    fn name(&self) -> &'static str {
        "value_translation_cleanup"
    }

    fn wants(&self, event_type: &str) -> bool {
        event_type == PRODUCT_FIELDS_UPDATED || event_type == CATEGORY_FIELDS_UPDATED
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        let removed: Vec<FieldKey> = event.data["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c.get("value").is_some_and(Value::is_null))
            .filter_map(|c| c["key"].as_str().and_then(|k| FieldKey::parse(k).ok()))
            .collect();
        if removed.is_empty() {
            return Ok(());
        }
        let unnamed = |field: &str| format!("event {} names no {field}", event.id);
        let variant = event.data["variant"]
            .as_str()
            .and_then(|v| v.parse::<VariantId>().ok());
        let (resource, owner, owner_type) = match event.event_type.as_str() {
            CATEGORY_FIELDS_UPDATED => {
                let category: CategoryId = event.data["category"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| unnamed("category"))?;
                let id = category.as_uuid();
                (
                    ResourceRef::new(ResourceKind::Category, id),
                    id,
                    OwnerType::Category,
                )
            }
            _ => {
                let product: ProductId = event.data["product"]
                    .as_str()
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| unnamed("product"))?;
                let resource = ResourceRef::new(ResourceKind::Product, product.as_uuid());
                match variant {
                    Some(v) => (resource, v.as_uuid(), OwnerType::Variant),
                    None => (resource, product.as_uuid(), OwnerType::Product),
                }
            }
        };
        let fail = |e: RepoError| e.to_string();
        let definitions: Vec<CustomFieldDefinition> = ports
            .custom_field_definitions()
            .list(Some(owner_type))
            .await
            .map_err(fail)?
            .into_iter()
            .filter(|d| removed.contains(&d.field_key()))
            .collect();
        if definitions.is_empty() {
            return Ok(());
        }
        let ids: Vec<CustomFieldDefinitionId> = definitions.iter().map(|d| d.id()).collect();
        let now = ports
            .custom_field_values()
            .get_many(&[owner], Some(&ids))
            .await
            .map_err(fail)?;
        // Each removed value's key, with the digest of the value set again since, if one
        // was.
        let gone: Vec<(String, Option<String>)> = ids
            .into_iter()
            .map(|d| {
                let key = match variant {
                    Some(v) => fields::custom_field::variant_value(v, d),
                    None => fields::custom_field::value(d),
                };
                let again = now
                    .iter()
                    .find(|row| row.definition == d)
                    .and_then(|row| row.value.as_str())
                    .map(source_digest);
                (key, again)
            })
            .collect();
        let repo = ports.translations();
        for mut translation in repo.for_resource(resource).await.map_err(fail)? {
            // A field translating the value set again since (written after it, or the
            // same text) is that value's: it stays. Any other translated the removed one.
            let stale: Vec<String> = gone
                .iter()
                .filter(|(key, again)| {
                    translation
                        .fields()
                        .get(key)
                        .is_some_and(|f| again.as_deref() != Some(f.source_digest.as_str()))
                })
                .map(|(key, _)| key.clone())
                .collect();
            if !translation.remove_fields(&stale) {
                continue;
            }
            let written = if translation.is_empty() {
                translation.mark_deleted();
                repo.delete(&mut translation).await
            } else {
                repo.update(&mut translation).await
            };
            // A translation written since it was read fails here and the delivery is
            // tried again, reading it afresh.
            written.map_err(fail)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_translations::FieldRule;

    use super::*;

    mod translatable {
        use hypha_core::CustomFieldDefinitionId;
        use hypha_custom_fields::{
            Access, AdminAccess, Capabilities, CustomFieldDefinition, DefinitionSpec, FieldType,
            Namespace, OwnerType, RawValidations, StorefrontAccess,
        };
        use uuid::Uuid;

        use crate::translations::translatable_value;

        struct Def {
            namespace: &'static str,
            owner: OwnerType,
            field_type: &'static str,
            admin: AdminAccess,
            storefront: StorefrontAccess,
            private: bool,
            pattern: Option<&'static str>,
        }

        const TEXT: Def = Def {
            namespace: "custom",
            owner: OwnerType::Product,
            field_type: "text",
            admin: AdminAccess::ReadWrite,
            storefront: StorefrontAccess::Read,
            private: false,
            pattern: None,
        };

        fn definition(d: Def) -> CustomFieldDefinition {
            CustomFieldDefinition::new(
                CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1)),
                Namespace::parse(d.namespace).unwrap(),
                DefinitionSpec {
                    owner_type: d.owner,
                    key: "field".to_string(),
                    field_type: FieldType::parse(d.field_type).unwrap(),
                    name: "Field".to_string(),
                    description: None,
                    validations: RawValidations {
                        pattern: d.pattern.map(str::to_string),
                        ..RawValidations::default()
                    },
                    access: Access {
                        admin: d.admin,
                        storefront: d.storefront,
                    },
                    private: d.private,
                    personal: false,
                    capabilities: Capabilities::default(),
                },
                0,
            )
            .unwrap()
        }

        #[test]
        fn prose_a_shopper_reads_on_the_catalog_is_translatable() {
            for owner in [OwnerType::Product, OwnerType::Variant, OwnerType::Category] {
                for field_type in ["text", "multiline_text", "rich_text"] {
                    let d = definition(Def {
                        owner,
                        field_type,
                        ..TEXT
                    });
                    assert!(translatable_value(&d), "{owner} {field_type}");
                }
            }
        }

        #[test]
        fn customers_orders_lists_and_other_types_are_not() {
            for owner in [OwnerType::Customer, OwnerType::Order] {
                assert!(!translatable_value(&definition(Def { owner, ..TEXT })));
            }
            // A choice's labels are its definition's own translation.
            for field_type in ["list.text", "url", "json", "integer"] {
                let d = definition(Def { field_type, ..TEXT });
                assert!(!translatable_value(&d), "{field_type}");
            }
        }

        #[test]
        fn hidden_private_patterned_or_read_only_fields_are_not() {
            let hidden = Def {
                storefront: StorefrontAccess::None,
                ..TEXT
            };
            assert!(!translatable_value(&definition(hidden)));
            let code = Def {
                pattern: Some("^[A-Z]+$"),
                ..TEXT
            };
            assert!(!translatable_value(&definition(code)));
            let private = Def {
                namespace: "app_erp",
                private: true,
                ..TEXT
            };
            assert!(!translatable_value(&definition(private)));
            // An app's field staff may only read: its text is the app's to say.
            let read_only = Def {
                namespace: "app_erp",
                admin: AdminAccess::Read,
                ..TEXT
            };
            assert!(!translatable_value(&definition(read_only)));
            let writable = Def {
                namespace: "app_erp",
                ..TEXT
            };
            assert!(translatable_value(&definition(writable)));
        }
    }

    fn limit(kind: ResourceKind, key: &str) -> usize {
        match kind.field(key).map(|(spec, _)| spec.rule) {
            Some(FieldRule::Text { max_chars }) => max_chars,
            Some(FieldRule::Html { max_bytes }) => max_bytes,
            Some(FieldRule::Value(rule)) => rule.max_length(),
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
                K::Product,
                "custom_fields.cfdef_1.value",
                hypha_custom_fields::limits::MAX_TEXT_CHARS,
            ),
            (
                K::Product,
                "variant_custom_fields.var_1.cfdef_1.value",
                hypha_custom_fields::limits::MAX_TEXT_CHARS,
            ),
            (
                K::Category,
                "custom_fields.cfdef_1.value",
                hypha_custom_fields::limits::MAX_TEXT_CHARS,
            ),
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
