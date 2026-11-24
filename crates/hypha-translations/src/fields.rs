//! The key of every translatable field, spelled once: the catalog's specs and whoever
//! reads or writes a translation (the host's sources, checkout's snapshots, search's
//! documents) build their keys here.

use std::fmt;

/// `<collection>.<child>.<name>`: a child's field, by the child's ID.
fn child(collection: &str, id: impl fmt::Display, name: &str) -> String {
    format!("{collection}.{id}.{name}")
}

pub mod product {
    use hypha_core::{OptionId, OptionValueId};

    pub const TITLE: &str = "title";
    pub const DESCRIPTION: &str = "description";
    pub(crate) const OPTIONS: &str = "options";
    pub(crate) const OPTION_NAME: &str = "name";
    pub(crate) const OPTION_VALUES: &str = "option_values";
    pub(crate) const OPTION_VALUE_LABEL: &str = "label";

    /// `options.<opt_…>.name`.
    pub fn option_name(option: OptionId) -> String {
        super::child(OPTIONS, option, OPTION_NAME)
    }

    /// `option_values.<optval_…>.label`.
    pub fn option_value_label(value: OptionValueId) -> String {
        super::child(OPTION_VALUES, value, OPTION_VALUE_LABEL)
    }
}

pub mod category {
    pub const NAME: &str = "name";
    pub const DESCRIPTION: &str = "description";
}

pub mod custom_field_definition {
    pub const NAME: &str = "name";
    pub const DESCRIPTION: &str = "description";
    pub(crate) const CHOICES: &str = "choices";
    pub(crate) const CHOICE_LABEL: &str = "label";

    /// `choices.<key>.label`, by the choice's key.
    pub fn choice_label(choice: &str) -> String {
        super::child(CHOICES, choice, CHOICE_LABEL)
    }
}

/// Custom field values, as fields of their owner's translation: a product's and a
/// category's own, and a variant's in its product's. Only a value of text a shopper reads
/// is translatable (the host decides which); a definition is named by its ID, which a
/// definition made again with the same key doesn't reuse.
pub mod custom_field {
    use hypha_core::{CustomFieldDefinitionId, VariantId};

    pub(crate) const CUSTOM_FIELDS: &str = "custom_fields";
    pub(crate) const VARIANT_CUSTOM_FIELDS: &str = "variant_custom_fields";
    pub(crate) const VALUE: &str = "value";

    /// `custom_fields.<cfdef_…>.value`: the owner's own value (a product's, a
    /// category's).
    pub fn value(definition: CustomFieldDefinitionId) -> String {
        super::child(CUSTOM_FIELDS, definition, VALUE)
    }

    /// `variant_custom_fields.<var_…>.<cfdef_…>.value`: a variant's value, in its
    /// product's translation.
    pub fn variant_value(variant: VariantId, definition: CustomFieldDefinitionId) -> String {
        super::child(
            VARIANT_CUSTOM_FIELDS,
            format_args!("{variant}.{definition}"),
            VALUE,
        )
    }
}

pub mod shipping_option {
    pub const NAME: &str = "name";
}

pub mod payment_method {
    pub const NAME: &str = "name";
    pub const INSTRUCTIONS: &str = "instructions";
}

pub mod discount {
    pub const TITLE: &str = "title";
}

pub mod checkout_rule {
    /// What the shopper is told when the rule holds.
    pub const MESSAGE: &str = "message";
}

pub mod asset {
    pub const ALT: &str = "alt";
}

pub mod store {
    pub const DESCRIPTION: &str = "description";
    /// The footer of the store's emails, from its notification settings.
    pub const EMAIL_FOOTER: &str = "email_footer";
}

pub mod search_facet {
    use std::fmt;

    pub(crate) const FACETS: &str = "facets";
    pub(crate) const FACET_LABEL: &str = "label";

    /// `facets.<facet key>.label`, by the facet's key (`vendor`, `option.color`).
    pub fn facet_label(facet: impl fmt::Display) -> String {
        super::child(FACETS, facet, FACET_LABEL)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{CustomFieldDefinitionId, OptionId, OptionValueId, VariantId};
    use uuid::Uuid;

    use super::*;
    use crate::catalog::ResourceKind as K;

    /// Every key a builder or constant gives is one its kind's catalog reads, and
    /// every field of the catalog has one.
    #[test]
    fn every_key_parses_under_its_kinds_catalog() {
        let id = Uuid::from_u128(1);
        let keys: Vec<(K, String)> = vec![
            (K::Product, product::TITLE.into()),
            (K::Product, product::DESCRIPTION.into()),
            (K::Product, product::option_name(OptionId::from_uuid(id))),
            (
                K::Product,
                product::option_value_label(OptionValueId::from_uuid(id)),
            ),
            (
                K::Product,
                custom_field::value(CustomFieldDefinitionId::from_uuid(id)),
            ),
            (
                K::Product,
                custom_field::variant_value(
                    VariantId::from_uuid(id),
                    CustomFieldDefinitionId::from_uuid(id),
                ),
            ),
            (K::Category, category::NAME.into()),
            (K::Category, category::DESCRIPTION.into()),
            (
                K::Category,
                custom_field::value(CustomFieldDefinitionId::from_uuid(id)),
            ),
            (
                K::CustomFieldDefinition,
                custom_field_definition::NAME.into(),
            ),
            (
                K::CustomFieldDefinition,
                custom_field_definition::DESCRIPTION.into(),
            ),
            (
                K::CustomFieldDefinition,
                custom_field_definition::choice_label("wool"),
            ),
            (K::ShippingOption, shipping_option::NAME.into()),
            (K::PaymentMethod, payment_method::NAME.into()),
            (K::PaymentMethod, payment_method::INSTRUCTIONS.into()),
            (K::Discount, discount::TITLE.into()),
            (K::CheckoutRule, checkout_rule::MESSAGE.into()),
            (K::Asset, asset::ALT.into()),
            (K::Store, store::DESCRIPTION.into()),
            (K::Store, store::EMAIL_FOOTER.into()),
            (K::SearchFacet, search_facet::facet_label("option.color")),
        ];
        for (kind, key) in &keys {
            assert!(kind.field(key).is_some(), "{kind} {key}");
        }
        let fields: usize = K::ALL.iter().map(|k| k.fields().len()).sum();
        assert_eq!(fields, keys.len());
    }

    #[test]
    fn a_childs_key_holds_its_prefixed_id() {
        let option = OptionId::from_uuid(Uuid::from_u128(1));
        assert_eq!(
            product::option_name(option),
            format!("options.{option}.name")
        );
        assert_eq!(
            K::Product.field(&product::option_name(option)).unwrap().1,
            Some(option.to_string().as_str())
        );
        assert_eq!(
            search_facet::facet_label("option.color"),
            "facets.option.color.label"
        );
    }

    #[test]
    fn a_variants_value_names_the_variant_and_the_definition() {
        let variant = VariantId::from_uuid(Uuid::from_u128(1));
        let definition = CustomFieldDefinitionId::from_uuid(Uuid::from_u128(2));
        let key = custom_field::variant_value(variant, definition);
        assert_eq!(
            key,
            format!("variant_custom_fields.{variant}.{definition}.value")
        );
        let (spec, child) = K::Product.field(&key).unwrap();
        assert_eq!(spec.collection, Some("variant_custom_fields"));
        assert_eq!(child, Some(format!("{variant}.{definition}").as_str()));
        // A product's own value isn't read as a variant's, nor the other way round.
        let own = custom_field::value(definition);
        assert_eq!(
            K::Product.field(&own).unwrap().0.collection,
            Some("custom_fields")
        );
        assert_eq!(K::Category.field(&key), None);
    }
}
