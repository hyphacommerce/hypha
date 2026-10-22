//! Custom fields as search facets: which fields may be facets, the facet values a stored
//! value gives, and loading a batch of products' values for their documents.

use std::collections::{BTreeMap, BTreeSet};

use hypha_core::{Ctx, ProductId, RepoError, Scope, VariantId};
use hypha_custom_fields::{
    BaseType, CallerKind, CustomFieldDefinition, DefinitionRepo, FieldCaller, FieldValue,
    HasCustomFieldDefinitions, HasCustomFieldValues, OwnerType, ReadAccess, ValueRepo, read_access,
};
use hypha_products::Product;
use hypha_search_index::FacetKey;

/// Whether `definition`'s values may be a search facet: a product or variant field that
/// is `filterable`, that a storefront reads (`access.storefront: read`, through
/// `hypha_custom_fields::read_access`, the one rule), of type text, choice, boolean or
/// integer, or a list of those.
pub fn facet_field(definition: &CustomFieldDefinition) -> bool {
    let owner = matches!(
        definition.owner_type(),
        OwnerType::Product | OwnerType::Variant
    );
    let typed = matches!(
        definition.field_type().base(),
        BaseType::Text | BaseType::Choice | BaseType::Boolean | BaseType::Integer
    );
    owner && typed && definition.capabilities().filterable && storefront_reads(definition)
}

/// What a storefront caller sees of the definition's values.
fn storefront_reads(definition: &CustomFieldDefinition) -> bool {
    let ctx = Ctx::new(hypha_core::Actor::System, vec![Scope::ProductsRead]);
    let kind = CallerKind::Storefront;
    read_access(definition, &FieldCaller::new(&ctx, &kind)) == ReadAccess::Visible
}

/// A value's facet values: text as written, a choice's key, `true`/`false`, an
/// integer's digits, one per item of a list (each once). None for other types.
pub fn facet_values(value: &FieldValue) -> Vec<String> {
    match value {
        FieldValue::Text(s) | FieldValue::Choice(s) => vec![s.clone()],
        FieldValue::Boolean(b) => vec![b.to_string()],
        FieldValue::Integer(n) => vec![n.to_string()],
        FieldValue::List(items) => {
            let mut seen = BTreeSet::new();
            items
                .iter()
                .flat_map(facet_values)
                .filter(|v| seen.insert(v.clone()))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// One product's facet field values: its own and each variant's, by full key
/// (`custom.material`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FacetFields {
    pub product: BTreeMap<String, Vec<String>>,
    pub variants: BTreeMap<VariantId, BTreeMap<String, Vec<String>>>,
}

impl FacetFields {
    /// A variant's document entries: its values, and its product's for the keys it has
    /// none of; sorted by key.
    pub fn for_variant(&self, variant: VariantId) -> Vec<(String, String)> {
        let mut merged = self.product.clone();
        if let Some(own) = self.variants.get(&variant) {
            for (key, values) in own {
                merged.insert(key.clone(), values.clone());
            }
        }
        merged
            .into_iter()
            .flat_map(|(key, values)| values.into_iter().map(move |v| (key.clone(), v)))
            .collect()
    }
}

/// The facet fields of `products` and their variants, from the definitions that may be
/// facets (`facet_field`), in one read of definitions and one of values. A stored value
/// that no longer reads as its definition's type is left out.
pub async fn load_facet_fields<S: HasCustomFieldDefinitions + HasCustomFieldValues>(
    ports: &S,
    products: &[&Product],
) -> Result<BTreeMap<ProductId, FacetFields>, RepoError> {
    let definitions: Vec<CustomFieldDefinition> = ports
        .custom_field_definitions()
        .list(None)
        .await?
        .into_iter()
        .filter(facet_field)
        .collect();
    if definitions.is_empty() || products.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut owner_of: BTreeMap<uuid::Uuid, (ProductId, Option<VariantId>)> = BTreeMap::new();
    for product in products {
        owner_of.insert(product.id().as_uuid(), (product.id(), None));
        for variant in product.variants() {
            owner_of.insert(variant.id().as_uuid(), (product.id(), Some(variant.id())));
        }
    }
    let owners: Vec<uuid::Uuid> = owner_of.keys().copied().collect();
    let ids: Vec<_> = definitions.iter().map(|d| d.id()).collect();
    let stored = ports
        .custom_field_values()
        .get_many(&owners, Some(&ids))
        .await?;

    let mut fields: BTreeMap<ProductId, FacetFields> = BTreeMap::new();
    for row in stored {
        let Some(definition) = definitions.iter().find(|d| d.id() == row.definition) else {
            continue;
        };
        let Some((product, variant)) = owner_of.get(&row.owner).copied() else {
            continue;
        };
        // A product definition's values belong to products, a variant one's to variants.
        let expected = match variant {
            None => OwnerType::Product,
            Some(_) => OwnerType::Variant,
        };
        if definition.owner_type() != expected {
            continue;
        }
        let Ok(value) = FieldValue::from_stored(definition.field_type(), &row.value) else {
            continue;
        };
        let values = facet_values(&value);
        if values.is_empty() {
            continue;
        }
        let key = definition.field_key().to_string();
        let entry = fields.entry(product).or_default();
        match variant {
            None => entry.product.insert(key, values),
            Some(v) => entry.variants.entry(v).or_default().insert(key, values),
        };
    }
    Ok(fields)
}

/// The value labels of the `choice` and `boolean` field facets among `facets`: a choice's
/// label by its key, from its definition, and "Yes"/"No" for `true`/`false` (filters and
/// documents use the stored values; labels are for display).
pub async fn choice_labels<S: HasCustomFieldDefinitions>(
    ports: &S,
    facets: &[FacetKey],
) -> Result<BTreeMap<FacetKey, BTreeMap<String, String>>, RepoError> {
    let wanted: Vec<&FacetKey> = facets
        .iter()
        .filter(|k| matches!(k, FacetKey::Field(_)))
        .collect();
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let definitions = ports.custom_field_definitions().list(None).await?;
    let mut labels: BTreeMap<FacetKey, BTreeMap<String, String>> = BTreeMap::new();
    for key in wanted {
        let FacetKey::Field(field) = key else {
            continue;
        };
        for definition in definitions
            .iter()
            .filter(|d| facet_field(d) && d.field_key().to_string() == *field)
        {
            match definition.field_type().base() {
                BaseType::Choice => {
                    let entry = labels.entry(key.clone()).or_default();
                    for choice in definition.validations().choices() {
                        entry.insert(choice.key.clone(), choice.label.clone());
                    }
                }
                // Documents hold `true`/`false`; shoppers read a yes or no.
                BaseType::Boolean => {
                    let entry = labels.entry(key.clone()).or_default();
                    entry.insert("true".to_string(), "Yes".to_string());
                    entry.insert("false".to_string(), "No".to_string());
                }
                _ => {}
            }
        }
    }
    Ok(labels)
}
