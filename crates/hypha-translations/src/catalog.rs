//! What can be translated: the closed set of resource kinds, each kind's fields and the
//! rule a translated value obeys (the rule of the field it replaces).

use std::{fmt, str::FromStr};

use hypha_core::{
    AssetId, CategoryId, CheckoutRuleId, CustomFieldDefinitionId, Description, DiscountId,
    ErrorCode, IdError, MAX_TEXT_LEN, PaymentMethodId, ProductId, Scope, ShippingOptionId, StoreId,
};
use uuid::Uuid;

/// A kind of resource with translatable text. The names are stored and are the API's:
/// never renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceKind {
    Product,
    Category,
    CustomFieldDefinition,
    ShippingOption,
    PaymentMethod,
    Discount,
    CheckoutRule,
    Asset,
    /// The store's own text: its description and its emails' footer.
    Store,
    /// The labels of the facets a storefront shows. They live in the store's one search
    /// settings record, so the resource is the store and each facet a child.
    SearchFacet,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} isn't a kind of resource with translations")]
pub struct UnknownKind(pub String);

impl ErrorCode for UnknownKind {
    fn code(&self) -> &'static str {
        "unknown_resource_kind"
    }
}

/// The rule of a translatable field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRule {
    /// Plain text, trimmed, at most this many characters.
    Text { max_chars: usize },
    /// HTML sanitised like core's `Description`, at most this many bytes as sent and as
    /// stored.
    Html { max_bytes: usize },
}

/// One translatable field of a kind: the resource's own (`title`), or one each child in
/// `collection` has (`options.<id>.name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    pub collection: Option<&'static str>,
    pub name: &'static str,
    pub rule: FieldRule,
}

const fn text(name: &'static str, max_chars: usize) -> FieldSpec {
    FieldSpec {
        collection: None,
        name,
        rule: FieldRule::Text { max_chars },
    }
}

const fn html(name: &'static str) -> FieldSpec {
    FieldSpec {
        collection: None,
        name,
        rule: FieldRule::Html {
            max_bytes: Description::MAX_LEN,
        },
    }
}

const fn child(collection: &'static str, name: &'static str, max_chars: usize) -> FieldSpec {
    FieldSpec {
        collection: Some(collection),
        name,
        rule: FieldRule::Text { max_chars },
    }
}

const PRODUCT: &[FieldSpec] = &[
    text("title", MAX_TEXT_LEN),
    html("description"),
    child("options", "name", MAX_TEXT_LEN),
    child("option_values", "label", MAX_TEXT_LEN),
];
const CATEGORY: &[FieldSpec] = &[text("name", MAX_TEXT_LEN), html("description")];
const CUSTOM_FIELD_DEFINITION: &[FieldSpec] = &[
    text("name", 255),
    text("description", 1000),
    child("choices", "label", MAX_TEXT_LEN),
];
const SHIPPING_OPTION: &[FieldSpec] = &[text("name", MAX_TEXT_LEN)];
const PAYMENT_METHOD: &[FieldSpec] = &[text("name", MAX_TEXT_LEN), text("instructions", 2000)];
const DISCOUNT: &[FieldSpec] = &[text("title", MAX_TEXT_LEN)];
const CHECKOUT_RULE: &[FieldSpec] = &[text("message", 300)];
const ASSET: &[FieldSpec] = &[text("alt", MAX_TEXT_LEN)];
const STORE: &[FieldSpec] = &[text("description", 500), text("email_footer", 1000)];
const SEARCH_FACET: &[FieldSpec] = &[child("facets", "label", MAX_TEXT_LEN)];

/// The longest field key: a child's ID is at most a custom field's full key.
pub const MAX_KEY_LEN: usize = 255;

impl ResourceKind {
    pub const ALL: [ResourceKind; 10] = [
        ResourceKind::Product,
        ResourceKind::Category,
        ResourceKind::CustomFieldDefinition,
        ResourceKind::ShippingOption,
        ResourceKind::PaymentMethod,
        ResourceKind::Discount,
        ResourceKind::CheckoutRule,
        ResourceKind::Asset,
        ResourceKind::Store,
        ResourceKind::SearchFacet,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Product => "product",
            ResourceKind::Category => "category",
            ResourceKind::CustomFieldDefinition => "custom_field_definition",
            ResourceKind::ShippingOption => "shipping_option",
            ResourceKind::PaymentMethod => "payment_method",
            ResourceKind::Discount => "discount",
            ResourceKind::CheckoutRule => "checkout_rule",
            ResourceKind::Asset => "asset",
            ResourceKind::Store => "store",
            ResourceKind::SearchFacet => "search_facet",
        }
    }

    /// The prefix of the kind's IDs at the API's edges (`prod`).
    pub fn id_prefix(self) -> &'static str {
        match self {
            ResourceKind::Product => ProductId::PREFIX,
            ResourceKind::Category => CategoryId::PREFIX,
            ResourceKind::CustomFieldDefinition => CustomFieldDefinitionId::PREFIX,
            ResourceKind::ShippingOption => ShippingOptionId::PREFIX,
            ResourceKind::PaymentMethod => PaymentMethodId::PREFIX,
            ResourceKind::Discount => DiscountId::PREFIX,
            ResourceKind::CheckoutRule => CheckoutRuleId::PREFIX,
            ResourceKind::Asset => AssetId::PREFIX,
            ResourceKind::Store | ResourceKind::SearchFacet => StoreId::PREFIX,
        }
    }

    /// What reading the resource itself takes: a caller needs these beside
    /// `translations:read` or `translations:write`, since a translation shows the source.
    pub fn read_scopes(self) -> &'static [Scope] {
        match self {
            ResourceKind::Product | ResourceKind::Category | ResourceKind::SearchFacet => {
                &[Scope::ProductsRead]
            }
            ResourceKind::CustomFieldDefinition => &[Scope::CustomFieldsRead],
            ResourceKind::ShippingOption => &[Scope::ShippingRead],
            ResourceKind::PaymentMethod => &[Scope::PaymentsRead],
            ResourceKind::Discount => &[Scope::DiscountsRead],
            ResourceKind::CheckoutRule => &[Scope::CheckoutRulesRead],
            ResourceKind::Asset => &[Scope::MediaRead],
            // The footer is the notification settings'.
            ResourceKind::Store => &[Scope::StoresRead, Scope::NotificationsRead],
        }
    }

    /// The kind's translatable fields. The limits are the domains' own (a host test
    /// holds them to each other).
    pub fn fields(self) -> &'static [FieldSpec] {
        match self {
            ResourceKind::Product => PRODUCT,
            ResourceKind::Category => CATEGORY,
            ResourceKind::CustomFieldDefinition => CUSTOM_FIELD_DEFINITION,
            ResourceKind::ShippingOption => SHIPPING_OPTION,
            ResourceKind::PaymentMethod => PAYMENT_METHOD,
            ResourceKind::Discount => DISCOUNT,
            ResourceKind::CheckoutRule => CHECKOUT_RULE,
            ResourceKind::Asset => ASSET,
            ResourceKind::Store => STORE,
            ResourceKind::SearchFacet => SEARCH_FACET,
        }
    }

    /// The field `key` names (`title`, `options.<id>.name`) and, for a child's, the
    /// child's ID as the key spells it. `None` for a key the kind doesn't have. A child's
    /// ID may itself hold dots (a facet's `option.Color`): it's what lies between the
    /// collection and the field's name.
    pub fn field(self, key: &str) -> Option<(&'static FieldSpec, Option<&str>)> {
        if key.len() > MAX_KEY_LEN || key.chars().any(char::is_control) {
            return None;
        }
        self.fields().iter().find_map(|spec| match spec.collection {
            None => (key == spec.name).then_some((spec, None)),
            Some(collection) => {
                let id = key
                    .strip_prefix(collection)?
                    .strip_prefix('.')?
                    .strip_suffix(spec.name)?
                    .strip_suffix('.')?;
                (!id.is_empty()).then_some((spec, Some(id)))
            }
        })
    }
}

/// The key of `child`'s field `name` in `collection`: `options.<child>.name`.
pub fn child_key(collection: &str, child: impl fmt::Display, name: &str) -> String {
    format!("{collection}.{child}.{name}")
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ResourceKind {
    type Err = UnknownKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ResourceKind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnknownKind(s.to_string()))
    }
}

/// A resource with translations: its kind and its UUID (the store's for the kinds that
/// are one per store).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub id: Uuid,
}

impl ResourceRef {
    pub fn new(kind: ResourceKind, id: Uuid) -> Self {
        Self { kind, id }
    }

    /// The resource `id` names at the API's edges (`prod_…` for a product).
    pub fn parse(kind: ResourceKind, id: &str) -> Result<Self, IdError> {
        let expected = kind.id_prefix();
        let rest = id
            .strip_prefix(expected)
            .and_then(|r| r.strip_prefix('_'))
            .ok_or(IdError::WrongPrefix { expected })?;
        let id = Uuid::parse_str(rest).map_err(|_| IdError::InvalidUuid)?;
        Ok(Self { kind, id })
    }

    /// Its ID as the API shows it (`prod_…`).
    pub fn id_string(&self) -> String {
        format!("{}_{}", self.kind.id_prefix(), self.id.simple())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_read_back_from_their_names() {
        for kind in ResourceKind::ALL {
            assert_eq!(kind.as_str().parse(), Ok(kind));
        }
        assert_eq!(
            "variant".parse::<ResourceKind>(),
            Err(UnknownKind("variant".to_string()))
        );
    }

    #[test]
    fn a_plain_key_names_the_resources_own_field() {
        let (spec, child) = ResourceKind::Product.field("title").unwrap();
        assert_eq!((spec.name, child), ("title", None));
        assert_eq!(ResourceKind::Product.field("name"), None);
        assert_eq!(ResourceKind::Category.field("title"), None);
    }

    #[test]
    fn a_childs_key_names_its_collection_its_id_and_the_field() {
        let (spec, child) = ResourceKind::Product.field("options.opt_1.name").unwrap();
        assert_eq!(
            (spec.collection, spec.name, child),
            (Some("options"), "name", Some("opt_1"))
        );
        assert_eq!(ResourceKind::Product.field("options..name"), None);
        assert_eq!(ResourceKind::Product.field("options.opt_1.label"), None);
        assert_eq!(ResourceKind::Product.field("options.opt_1"), None);
    }

    #[test]
    fn a_childs_id_may_hold_dots() {
        let (_, child) = ResourceKind::SearchFacet
            .field("facets.option.Color.label")
            .unwrap();
        assert_eq!(child, Some("option.Color"));
    }

    #[test]
    fn a_key_is_at_most_255_bytes_without_control_characters() {
        let long = format!("choices.{}.label", "a".repeat(MAX_KEY_LEN));
        assert_eq!(ResourceKind::CustomFieldDefinition.field(&long), None);
        assert_eq!(
            ResourceKind::CustomFieldDefinition.field("choices.a\nb.label"),
            None
        );
    }

    #[test]
    fn a_resource_reads_its_prefixed_id() {
        let id = Uuid::from_u128(7);
        let resource = ResourceRef::new(ResourceKind::Product, id);
        assert_eq!(
            ResourceRef::parse(ResourceKind::Product, &resource.id_string()),
            Ok(resource)
        );
        assert_eq!(
            ResourceRef::parse(ResourceKind::Category, &resource.id_string()),
            Err(IdError::WrongPrefix { expected: "pcat" })
        );
        assert_eq!(
            ResourceRef::parse(ResourceKind::Product, "prod_x"),
            Err(IdError::InvalidUuid)
        );
    }
}
