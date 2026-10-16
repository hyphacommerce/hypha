//! Editing a product after it's created (design note 0014): title and handle, options
//! and their values, variants, media. A child module of `product`, so it sees the
//! aggregate's private fields. Every method checks everything before it changes
//! anything, so a refused edit leaves the product as it was.

use std::collections::{HashMap, HashSet};

use hypha_core::{
    AssetId, IdGenerator, OptionId, OptionValueId, Price, VariantId, Weight, optional_text,
    required_text,
};

use super::{Product, validate_options};
use crate::{
    Description, Handle, OptionValue, ProductError, ProductOption, content,
    variant::{Variant, VariantPatch},
};

/// A variant as a request lists it in a whole list (`Product::set_variants`): an
/// existing one by `id`, or a new one without. Every field is given, as in a `PUT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDraft {
    pub id: Option<VariantId>,
    pub sku: Option<String>,
    pub price: Price,
    pub selections: Vec<OptionValueId>,
    pub weight: Option<Weight>,
    pub requires_shipping: bool,
}

/// An option as a request lists it: an existing one by `id`, or a new one without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDraft {
    pub id: Option<OptionId>,
    pub name: String,
    pub values: Vec<OptionValueDraft>,
}

/// An option value as a request lists it: an existing one by `id`, or a new one without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionValueDraft {
    pub id: Option<OptionValueId>,
    pub label: String,
}

impl Product {
    pub fn rename(&mut self, title: &str) -> Result<(), ProductError> {
        let title = required_text("title", title)?;
        if title != self.title {
            self.title = title;
            self.changes.field("title");
        }
        Ok(())
    }

    /// Sets or clears the description; HTML is sanitised (`Description::new`).
    pub fn set_description(&mut self, html: Option<&str>) -> Result<(), ProductError> {
        let description = html.map(Description::new).transpose()?.flatten();
        if description != self.description {
            self.description = description;
            self.changes.field("description");
        }
        Ok(())
    }

    pub fn set_vendor(&mut self, vendor: Option<String>) -> Result<(), ProductError> {
        let vendor = optional_text("vendor", vendor)?;
        if vendor != self.vendor {
            self.vendor = vendor;
            self.changes.field("vendor");
        }
        Ok(())
    }

    pub fn set_product_type(&mut self, product_type: Option<String>) -> Result<(), ProductError> {
        let product_type = optional_text("product type", product_type)?;
        if product_type != self.product_type {
            self.product_type = product_type;
            self.changes.field("product_type");
        }
        Ok(())
    }

    /// Replaces the tags (`content::tags` cleans them).
    pub fn set_tags(&mut self, tags: Vec<String>) -> Result<(), ProductError> {
        let tags = content::tags(tags)?;
        if tags != self.tags {
            self.tags = tags;
            self.changes.field("tags");
        }
        Ok(())
    }

    /// Uniqueness is the repository's key (`keys::HANDLE`), as on create.
    pub fn set_handle(&mut self, handle: Handle) {
        if handle != self.handle {
            self.handle = handle;
            self.changes.field("handle");
        }
    }

    /// Replaces the options with `drafts`, in order (design note 0014, section 3).
    /// Drafts with an `id` keep that option or value (renamed, moved); the others are new
    /// and get IDs from `ids`. Then, for the variants:
    ///
    /// 1. those in `remove` are removed;
    /// 2. any other that selects a value removed from an option that stays fails the
    ///    edit (`OptionValueInUse`, naming them), so nothing disappears unasked;
    /// 3. a removed option drops out of every selection, and two variants that become
    ///    the same combination fail it (`VariantsCollide`);
    /// 4. a new option gives every variant its first value.
    pub fn set_options(
        &mut self,
        drafts: Vec<OptionDraft>,
        remove: &[VariantId],
        ids: &impl IdGenerator,
    ) -> Result<(), ProductError> {
        let options = self.resolve_options(drafts, ids)?;

        let mut removing = HashSet::new();
        for id in remove {
            if self.variant(*id).is_none() {
                return Err(ProductError::UnknownVariant(*id));
            }
            removing.insert(*id);
        }

        // Positions in the old option list, by option, to carry selections over.
        let old_position: HashMap<OptionId, usize> = self
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id(), i))
            .collect();

        let orphaned: Vec<VariantId> = self
            .variants
            .iter()
            .filter(|v| !removing.contains(&v.id()))
            .filter(|v| {
                options.iter().any(|option| {
                    old_position
                        .get(&option.id())
                        .and_then(|i| v.selections().get(*i))
                        .is_some_and(|selected| !option.has_value(*selected))
                })
            })
            .map(|v| v.id())
            .collect();
        if !orphaned.is_empty() {
            return Err(ProductError::OptionValueInUse { variants: orphaned });
        }

        let mut variants = Vec::with_capacity(self.variants.len());
        // Variants whose selections the new options change, for their events.
        let mut changed = Vec::new();
        let mut combinations: HashMap<Vec<OptionValueId>, VariantId> = HashMap::new();
        for variant in self.variants.iter().filter(|v| !removing.contains(&v.id())) {
            // Every option has a value (`validate_options`) and every selection list has
            // one per old option (the aggregate's invariant), so each lookup finds one.
            let selections: Vec<OptionValueId> = options
                .iter()
                .filter_map(|option| match old_position.get(&option.id()) {
                    Some(i) => variant.selections().get(*i).copied(),
                    None => option.values().first().map(|v| v.id()),
                })
                .collect();
            if let Some(first) = combinations.insert(selections.clone(), variant.id()) {
                return Err(ProductError::VariantsCollide {
                    first,
                    second: variant.id(),
                });
            }
            let mut variant = variant.clone();
            if variant.selections() != selections.as_slice() {
                changed.push(variant.id());
            }
            variant.set_selections(selections);
            variants.push(variant);
        }
        if variants.is_empty() {
            return Err(ProductError::NoVariants);
        }

        self.options = options;
        self.variants = variants;
        self.changes.field("options");
        for id in remove {
            self.changes.variant_removed(*id);
        }
        for id in changed {
            self.changes.variant_field(id, "selections");
        }
        Ok(())
    }

    /// The options `drafts` describe, with IDs for the new ones, checked.
    fn resolve_options(
        &self,
        drafts: Vec<OptionDraft>,
        ids: &impl IdGenerator,
    ) -> Result<Vec<ProductOption>, ProductError> {
        let mut listed_options = HashSet::new();
        let mut listed_values = HashSet::new();
        let mut options = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let existing = match draft.id {
                Some(id) => {
                    let existing = self
                        .options
                        .iter()
                        .find(|o| o.id() == id)
                        .ok_or(ProductError::UnknownOption(id))?;
                    if !listed_options.insert(id) {
                        return Err(ProductError::DuplicateOption(id));
                    }
                    Some(existing)
                }
                None => None,
            };
            let mut values = Vec::with_capacity(draft.values.len());
            for value in draft.values {
                let id = match value.id {
                    Some(id) => {
                        // A value stays in its option: moving "Red" from Color to Size
                        // would quietly change what every variant using it means.
                        if !existing.is_some_and(|o| o.has_value(id)) {
                            return Err(ProductError::UnknownOptionValue(id));
                        }
                        if !listed_values.insert(id) {
                            return Err(ProductError::DuplicateOptionValue(id));
                        }
                        id
                    }
                    None => OptionValueId::from_uuid(ids.new_uuid()),
                };
                values.push(OptionValue::new(
                    id,
                    required_text("option value", &value.label)?,
                ));
            }
            let id = draft
                .id
                .unwrap_or_else(|| OptionId::from_uuid(ids.new_uuid()));
            let name = required_text("option name", &draft.name)?;
            options.push(ProductOption::new(id, name, values));
        }
        validate_options(&options)?;
        Ok(options)
    }

    /// Changes the fields `patch` sets. New selections follow `add_variant`'s rules.
    pub fn update_variant(
        &mut self,
        id: VariantId,
        patch: VariantPatch,
    ) -> Result<(), ProductError> {
        let index = self
            .variants
            .iter()
            .position(|v| v.id() == id)
            .ok_or(ProductError::VariantNotFound(id))?;
        if let Some(selections) = &patch.selections {
            self.check_selections(selections, Some(id))?;
        }
        let sku = match patch.sku {
            Some(sku) => {
                let sku = optional_text("sku", sku)?;
                self.check_sku(sku.as_deref(), Some(id))?;
                Some(sku)
            }
            None => None,
        };

        let variant = &mut self.variants[index];
        let mut changed = Vec::new();
        if let Some(sku) = sku
            && variant.sku() != sku.as_deref()
        {
            variant.set_sku(sku);
            changed.push("sku");
        }
        if let Some(price) = patch.price
            && variant.base_price() != price
        {
            variant.set_base_price(price);
            changed.push("price");
        }
        if let Some(selections) = patch.selections
            && variant.selections() != selections.as_slice()
        {
            variant.set_selections(selections);
            changed.push("selections");
        }
        if let Some(weight) = patch.weight
            && variant.weight() != weight
        {
            variant.set_weight(weight);
            changed.push("weight");
        }
        if let Some(requires) = patch.requires_shipping
            && variant.requires_shipping() != requires
        {
            variant.set_requires_shipping(requires);
            changed.push("requires_shipping");
        }
        for field in changed {
            self.changes.variant_field(id, field);
        }
        Ok(())
    }

    /// Replaces the variant list with `drafts`, in order (design note 0021, Q1): a draft
    /// with an ID updates that variant, one without adds a variant (IDs from `ids`),
    /// and a variant left out is removed. The rules are `add_variant`'s, over the whole
    /// list: 1 to `MAX_VARIANTS` variants, each naming one value per option, no two
    /// with the same combination or SKU. A variant keeps its media; new ones have none.
    pub fn set_variants(
        &mut self,
        drafts: Vec<VariantDraft>,
        ids: &impl IdGenerator,
    ) -> Result<(), ProductError> {
        if drafts.is_empty() {
            return Err(ProductError::NoVariants);
        }
        if drafts.len() > Self::MAX_VARIANTS {
            return Err(ProductError::TooManyVariants {
                max: Self::MAX_VARIANTS,
            });
        }
        let mut listed = HashSet::new();
        let mut combinations = HashSet::new();
        let mut skus = HashSet::new();
        let mut variants = Vec::with_capacity(drafts.len());
        for draft in drafts {
            if draft.selections.len() != self.options.len() {
                return Err(ProductError::SelectionCountMismatch {
                    expected: self.options.len(),
                    got: draft.selections.len(),
                });
            }
            for (option, selection) in self.options.iter().zip(&draft.selections) {
                if !option.has_value(*selection) {
                    return Err(ProductError::UnknownOptionValue(*selection));
                }
            }
            if !combinations.insert(draft.selections.clone()) {
                return Err(ProductError::DuplicateCombination);
            }
            let sku = optional_text("sku", draft.sku)?;
            if let Some(sku) = &sku
                && !skus.insert(sku.clone())
            {
                return Err(ProductError::DuplicateSku(sku.clone()));
            }
            let mut variant = match draft.id {
                Some(id) => {
                    if !listed.insert(id) {
                        return Err(ProductError::DuplicateVariantId(id));
                    }
                    self.variant(id)
                        .cloned()
                        .ok_or(ProductError::VariantNotFound(id))?
                }
                None => Variant::new(
                    VariantId::from_uuid(ids.new_uuid()),
                    None,
                    draft.price,
                    Vec::new(),
                ),
            };
            variant.set_sku(sku);
            variant.set_base_price(draft.price);
            variant.set_selections(draft.selections);
            variant.set_weight(draft.weight);
            variant.set_requires_shipping(draft.requires_shipping);
            variants.push(variant);
        }

        // Everything checked: record what changed, then swap the list in.
        let before: Vec<VariantId> = self.variants.iter().map(|v| v.id()).collect();
        for old in &self.variants {
            if !variants.iter().any(|v| v.id() == old.id()) {
                self.changes.variant_removed(old.id());
            }
        }
        for new in &variants {
            let Some(old) = self.variant(new.id()) else {
                self.changes.variant_added(new.id());
                continue;
            };
            let mut changed = Vec::new();
            if old.sku() != new.sku() {
                changed.push("sku");
            }
            if old.base_price() != new.base_price() {
                changed.push("price");
            }
            if old.selections() != new.selections() {
                changed.push("selections");
            }
            if old.weight() != new.weight() {
                changed.push("weight");
            }
            if old.requires_shipping() != new.requires_shipping() {
                changed.push("requires_shipping");
            }
            for field in changed {
                self.changes.variant_field(new.id(), field);
            }
        }
        let kept: Vec<VariantId> = before
            .iter()
            .copied()
            .filter(|id| variants.iter().any(|v| v.id() == *id))
            .collect();
        let kept_in_new_order: Vec<VariantId> = variants
            .iter()
            .map(|v| v.id())
            .filter(|id| kept.contains(id))
            .collect();
        if kept != kept_in_new_order {
            self.changes.field("variant_order");
        }
        self.variants = variants;
        Ok(())
    }

    /// Removes a variant; the last one stays (`NoVariants`).
    pub fn remove_variant(&mut self, id: VariantId) -> Result<(), ProductError> {
        let index = self
            .variants
            .iter()
            .position(|v| v.id() == id)
            .ok_or(ProductError::VariantNotFound(id))?;
        if self.variants.len() == 1 {
            return Err(ProductError::NoVariants);
        }
        self.variants.remove(index);
        self.changes.variant_removed(id);
        Ok(())
    }

    /// Replaces the media list (the first is featured). Variants showing an item that
    /// is no longer in it show nothing.
    pub fn set_media(&mut self, media: Vec<AssetId>) -> Result<(), ProductError> {
        if media.len() > Self::MAX_MEDIA {
            return Err(ProductError::TooManyMedia {
                max: Self::MAX_MEDIA,
            });
        }
        let mut seen = HashSet::new();
        if let Some(twice) = media.iter().find(|id| !seen.insert(**id)) {
            return Err(ProductError::DuplicateMedia(*twice));
        }
        for variant in &mut self.variants {
            if variant.media().is_some_and(|id| !media.contains(&id)) {
                variant.set_media(None);
                self.changes.variant_field(variant.id(), "media");
            }
        }
        if media != self.media {
            self.media = media;
            self.changes.field("media");
        }
        Ok(())
    }

    /// Which item of the media list a variant shows, or none.
    pub fn set_variant_media(
        &mut self,
        id: VariantId,
        media: Option<AssetId>,
    ) -> Result<(), ProductError> {
        if let Some(asset) = media
            && !self.media.contains(&asset)
        {
            return Err(ProductError::MediaNotInProduct(asset));
        }
        let variant = self
            .variants
            .iter_mut()
            .find(|v| v.id() == id)
            .ok_or(ProductError::VariantNotFound(id))?;
        if variant.media() != media {
            variant.set_media(media);
            self.changes.variant_field(id, "media");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use hypha_core::{Currency, Money, Price, ProductId, TextError, Weight};
    use uuid::Uuid;

    use super::*;
    use crate::NewVariant;

    /// IDs from 1000 up, apart from the fixtures' small numbers.
    struct Ids(AtomicU64);

    impl Default for Ids {
        fn default() -> Self {
            Self(AtomicU64::new(1000))
        }
    }

    impl IdGenerator for Ids {
        fn new_uuid(&self) -> Uuid {
            Uuid::from_u128(u128::from(self.0.fetch_add(1, Ordering::SeqCst)))
        }
    }

    fn val(n: u128) -> OptionValueId {
        OptionValueId::from_uuid(Uuid::from_u128(n))
    }
    fn var(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }
    fn opt(n: u128) -> OptionId {
        OptionId::from_uuid(Uuid::from_u128(n))
    }
    fn asset(n: u128) -> AssetId {
        AssetId::from_uuid(Uuid::from_u128(n))
    }
    fn price(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap()
    }

    fn new_var(id: u128, selections: Vec<OptionValueId>) -> NewVariant {
        NewVariant {
            id: var(id),
            sku: None,
            price: price(1000),
            selections,
        }
    }

    /// Size (1): S=11, M=12; Color (2): Red=21, Blue=22. Variants S/Red (1), M/Blue (2).
    fn shirt() -> Product {
        let size = ProductOption::new(
            opt(1),
            "Size".to_string(),
            vec![
                OptionValue::new(val(11), "S".to_string()),
                OptionValue::new(val(12), "M".to_string()),
            ],
        );
        let color = ProductOption::new(
            opt(2),
            "Color".to_string(),
            vec![
                OptionValue::new(val(21), "Red".to_string()),
                OptionValue::new(val(22), "Blue".to_string()),
            ],
        );
        let mut p = Product::new_configurable(
            ProductId::from_uuid(Uuid::from_u128(100)),
            Handle::new("shirt").unwrap(),
            "Shirt".to_string(),
            vec![size, color],
            new_var(1, vec![val(11), val(21)]),
        )
        .unwrap();
        p.add_variant(new_var(2, vec![val(12), val(22)])).unwrap();
        p
    }

    fn simple() -> Product {
        Product::new_simple(
            ProductId::from_uuid(Uuid::from_u128(200)),
            Handle::new("mug").unwrap(),
            "Mug".to_string(),
            var(1),
            price(1000),
        )
        .unwrap()
    }

    fn existing(id: u128, name: &str, values: &[(u128, &str)]) -> OptionDraft {
        OptionDraft {
            id: Some(opt(id)),
            name: name.to_string(),
            values: values
                .iter()
                .map(|(id, label)| OptionValueDraft {
                    id: Some(val(*id)),
                    label: label.to_string(),
                })
                .collect(),
        }
    }

    fn new_option(name: &str, labels: &[&str]) -> OptionDraft {
        OptionDraft {
            id: None,
            name: name.to_string(),
            values: labels
                .iter()
                .map(|label| OptionValueDraft {
                    id: None,
                    label: label.to_string(),
                })
                .collect(),
        }
    }

    fn size() -> OptionDraft {
        existing(1, "Size", &[(11, "S"), (12, "M")])
    }
    fn color() -> OptionDraft {
        existing(2, "Color", &[(21, "Red"), (22, "Blue")])
    }

    fn selections(p: &Product) -> Vec<Vec<OptionValueId>> {
        p.variants()
            .iter()
            .map(|v| v.selections().to_vec())
            .collect()
    }

    // ---- title and handle ----

    #[test]
    fn rename_trims_and_requires_a_title() {
        let mut p = simple();
        assert_eq!(p.rename(" Cup "), Ok(()));
        assert_eq!(p.title(), "Cup");
        assert_eq!(
            p.rename(" "),
            Err(ProductError::Text(TextError::Missing("title")))
        );
        assert_eq!(p.title(), "Cup");
    }

    // ---- set_options ----

    #[test]
    fn renaming_options_and_values_keeps_ids_and_selections() {
        let mut p = shirt();
        let drafts = vec![
            existing(1, "Size ", &[(11, "Small"), (12, "Medium")]),
            color(),
        ];
        assert_eq!(p.set_options(drafts, &[], &Ids::default()), Ok(()));
        assert_eq!(p.options()[0].name(), "Size");
        assert_eq!(p.options()[0].values()[0].label(), "Small");
        assert_eq!(p.variant_title(var(1)), Some("Small / Red".to_string()));
    }

    #[test]
    fn reordering_options_reorders_every_selection() {
        let mut p = shirt();
        assert_eq!(
            p.set_options(vec![color(), size()], &[], &Ids::default()),
            Ok(())
        );
        assert_eq!(
            selections(&p),
            vec![vec![val(21), val(11)], vec![val(22), val(12)]]
        );
        assert_eq!(p.variant_title(var(2)), Some("Blue / M".to_string()));
    }

    #[test]
    fn a_new_option_gives_every_variant_its_first_value() {
        let mut p = shirt();
        let drafts = vec![
            size(),
            color(),
            new_option("Material", &["Cotton", "Linen"]),
        ];
        assert_eq!(p.set_options(drafts, &[], &Ids::default()), Ok(()));
        let cotton = p.options()[2].values()[0].id();
        assert_eq!(
            selections(&p),
            vec![
                vec![val(11), val(21), cotton],
                vec![val(12), val(22), cotton]
            ]
        );
    }

    #[test]
    fn a_simple_product_gets_its_first_option() {
        let mut p = simple();
        let drafts = vec![new_option("Size", &["S", "M"])];
        assert_eq!(p.set_options(drafts, &[], &Ids::default()), Ok(()));
        assert_eq!(p.variant_title(var(1)), Some("S".to_string()));
    }

    #[test]
    fn new_values_and_options_get_ids_from_the_generator() {
        let mut p = shirt();
        let drafts = vec![
            existing(1, "Size", &[(11, "S"), (12, "M")]),
            OptionDraft {
                values: vec![
                    OptionValueDraft {
                        id: Some(val(21)),
                        label: "Red".to_string(),
                    },
                    OptionValueDraft {
                        id: Some(val(22)),
                        label: "Blue".to_string(),
                    },
                    OptionValueDraft {
                        id: None,
                        label: "Green".to_string(),
                    },
                ],
                ..color()
            },
        ];
        assert_eq!(p.set_options(drafts, &[], &Ids::default()), Ok(()));
        assert_eq!(p.options()[1].values()[2].id(), val(1000));
    }

    #[test]
    fn removing_a_used_value_names_the_variants_that_use_it() {
        let mut p = shirt();
        let before = p.clone();
        let drafts = vec![existing(1, "Size", &[(11, "S")]), color()];
        assert_eq!(
            p.set_options(drafts, &[], &Ids::default()),
            Err(ProductError::OptionValueInUse {
                variants: vec![var(2)]
            })
        );
        assert_eq!(p, before);
    }

    #[test]
    fn removing_a_used_value_removes_the_variants_named_with_it() {
        let mut p = shirt();
        let drafts = vec![existing(1, "Size", &[(11, "S")]), color()];
        assert_eq!(p.set_options(drafts, &[var(2)], &Ids::default()), Ok(()));
        assert_eq!(
            p.variants().iter().map(|v| v.id()).collect::<Vec<_>>(),
            vec![var(1)]
        );
    }

    #[test]
    fn removing_an_unused_value_needs_nothing_else() {
        let mut p = shirt();
        p.remove_variant(var(2)).unwrap();
        let drafts = vec![existing(1, "Size", &[(11, "S")]), color()];
        assert_eq!(p.set_options(drafts, &[], &Ids::default()), Ok(()));
    }

    #[test]
    fn a_variant_named_for_removal_must_be_the_products() {
        let mut p = shirt();
        assert_eq!(
            p.set_options(vec![size(), color()], &[var(9)], &Ids::default()),
            Err(ProductError::UnknownVariant(var(9)))
        );
    }

    #[test]
    fn removing_an_option_drops_it_from_selections() {
        let mut p = shirt();
        assert_eq!(p.set_options(vec![size()], &[], &Ids::default()), Ok(()));
        assert_eq!(selections(&p), vec![vec![val(11)], vec![val(12)]]);
    }

    #[test]
    fn removing_an_option_refuses_variants_that_become_the_same() {
        let mut p = shirt();
        p.add_variant(new_var(3, vec![val(11), val(22)])).unwrap(); // S / Blue
        let before = p.clone();
        assert_eq!(
            p.set_options(vec![size()], &[], &Ids::default()),
            Err(ProductError::VariantsCollide {
                first: var(1),
                second: var(3)
            })
        );
        assert_eq!(p, before);
    }

    #[test]
    fn removing_every_option_leaves_a_single_variant_product() {
        let mut p = shirt();
        assert_eq!(p.set_options(vec![], &[var(2)], &Ids::default()), Ok(()));
        assert!(p.options().is_empty());
        assert_eq!(selections(&p), vec![Vec::<OptionValueId>::new()]);
    }

    #[test]
    fn removing_every_variant_is_refused() {
        let mut p = shirt();
        assert_eq!(
            p.set_options(vec![size(), color()], &[var(1), var(2)], &Ids::default()),
            Err(ProductError::NoVariants)
        );
    }

    #[test]
    fn an_option_id_must_be_the_products() {
        let mut p = shirt();
        assert_eq!(
            p.set_options(
                vec![existing(9, "Size", &[(11, "S")])],
                &[],
                &Ids::default()
            ),
            Err(ProductError::UnknownOption(opt(9)))
        );
    }

    #[test]
    fn an_option_listed_twice_is_refused() {
        let mut p = shirt();
        assert_eq!(
            p.set_options(
                vec![size(), existing(1, "Size 2", &[])],
                &[],
                &Ids::default()
            ),
            Err(ProductError::DuplicateOption(opt(1)))
        );
    }

    #[test]
    fn a_value_cant_move_to_another_option() {
        let mut p = shirt();
        let drafts = vec![
            existing(1, "Size", &[(11, "S"), (12, "M"), (21, "Red")]),
            color(),
        ];
        assert_eq!(
            p.set_options(drafts, &[], &Ids::default()),
            Err(ProductError::UnknownOptionValue(val(21)))
        );
    }

    #[test]
    fn a_value_listed_twice_is_refused() {
        let mut p = shirt();
        let drafts = vec![existing(1, "Size", &[(11, "S"), (11, "Small")]), color()];
        assert_eq!(
            p.set_options(drafts, &[], &Ids::default()),
            Err(ProductError::DuplicateOptionValue(val(11)))
        );
    }

    #[test]
    fn option_names_are_unique_ignoring_case() {
        let mut p = shirt();
        let drafts = vec![size(), color(), new_option("size", &["XL"])];
        assert_eq!(
            p.set_options(drafts, &[], &Ids::default()),
            Err(ProductError::DuplicateOptionName("size".to_string()))
        );
    }

    #[test]
    fn labels_are_unique_within_an_option_ignoring_case() {
        let mut p = shirt();
        let drafts = vec![size(), new_option("Material", &["Cotton", "cotton"])];
        assert_eq!(
            p.set_options(drafts, &[], &Ids::default()),
            Err(ProductError::DuplicateOptionLabel {
                option: "Material".to_string(),
                label: "cotton".to_string()
            })
        );
    }

    #[test]
    fn names_and_labels_are_required() {
        let mut p = simple();
        assert_eq!(
            p.set_options(vec![new_option(" ", &["S"])], &[], &Ids::default()),
            Err(ProductError::Text(TextError::Missing("option name")))
        );
        assert_eq!(
            p.set_options(vec![new_option("Size", &[""])], &[], &Ids::default()),
            Err(ProductError::Text(TextError::Missing("option value")))
        );
    }

    #[test]
    fn an_option_needs_a_value() {
        let mut p = simple();
        assert_eq!(
            p.set_options(vec![new_option("Size", &[])], &[], &Ids::default()),
            Err(ProductError::EmptyOption(opt(1000)))
        );
    }

    // ---- variants ----

    #[test]
    fn a_sku_is_unique_within_the_product() {
        let mut p = shirt();
        p.update_variant(
            var(1),
            VariantPatch {
                sku: Some(Some("SHIRT-S".to_string())),
                ..VariantPatch::default()
            },
        )
        .unwrap();
        let taken = VariantPatch {
            sku: Some(Some(" SHIRT-S ".to_string())),
            ..VariantPatch::default()
        };
        assert_eq!(
            p.update_variant(var(2), taken.clone()),
            Err(ProductError::DuplicateSku("SHIRT-S".to_string()))
        );
        // Setting a variant's own SKU again is fine.
        assert_eq!(p.update_variant(var(1), taken), Ok(()));
    }

    #[test]
    fn a_blank_sku_clears_it() {
        let mut p = shirt();
        let patch = |sku: &str| VariantPatch {
            sku: Some(Some(sku.to_string())),
            ..VariantPatch::default()
        };
        p.update_variant(var(1), patch("A")).unwrap();
        assert_eq!(p.update_variant(var(1), patch("  ")), Ok(()));
        assert_eq!(p.variant(var(1)).unwrap().sku(), None);
    }

    #[test]
    fn updating_changes_only_the_fields_given() {
        let mut p = shirt();
        let patch = VariantPatch {
            price: Some(price(2500)),
            ..VariantPatch::default()
        };
        assert_eq!(p.update_variant(var(1), patch), Ok(()));
        let v = p.variant(var(1)).unwrap();
        assert_eq!(v.base_price(), price(2500));
        assert_eq!(v.selections(), &[val(11), val(21)]);
    }

    #[test]
    fn new_selections_must_be_a_free_combination() {
        let mut p = shirt();
        let to = |selections| VariantPatch {
            selections: Some(selections),
            ..VariantPatch::default()
        };
        assert_eq!(
            p.update_variant(var(1), to(vec![val(12), val(22)])),
            Err(ProductError::DuplicateCombination)
        );
        // Its own combination is free for itself.
        assert_eq!(p.update_variant(var(1), to(vec![val(11), val(21)])), Ok(()));
        assert_eq!(p.update_variant(var(1), to(vec![val(12), val(21)])), Ok(()));
        assert_eq!(p.variant_title(var(1)), Some("M / Red".to_string()));
    }

    #[test]
    fn a_variant_has_no_weight_and_requires_shipping_until_told_otherwise() {
        let mut p = shirt();
        let v = p.variant(var(1)).unwrap();
        assert_eq!((v.weight(), v.requires_shipping()), (None, true));
        let patch = VariantPatch {
            weight: Some(Some(Weight::from_grams(320))),
            requires_shipping: Some(false),
            ..VariantPatch::default()
        };
        assert_eq!(p.update_variant(var(1), patch), Ok(()));
        let v = p.variant(var(1)).unwrap();
        assert_eq!(
            (v.weight(), v.requires_shipping()),
            (Some(Weight::from_grams(320)), false)
        );

        let clear = VariantPatch {
            weight: Some(None),
            ..VariantPatch::default()
        };
        assert_eq!(p.update_variant(var(1), clear), Ok(()));
        assert_eq!(p.variant(var(1)).unwrap().weight(), None);
    }

    #[test]
    fn updating_an_unknown_variant_fails() {
        let mut p = shirt();
        assert_eq!(
            p.update_variant(var(9), VariantPatch::default()),
            Err(ProductError::VariantNotFound(var(9)))
        );
    }

    #[test]
    fn the_last_variant_cant_be_removed() {
        let mut p = shirt();
        assert_eq!(p.remove_variant(var(1)), Ok(()));
        assert_eq!(p.remove_variant(var(2)), Err(ProductError::NoVariants));
        assert_eq!(
            p.remove_variant(var(1)),
            Err(ProductError::VariantNotFound(var(1)))
        );
    }

    #[test]
    fn a_product_has_at_most_max_variants() {
        let values: Vec<OptionValue> = (0..=Product::MAX_VARIANTS as u128)
            .map(|n| OptionValue::new(val(10_000 + n), format!("V{n}")))
            .collect();
        let option = ProductOption::new(opt(1), "Size".to_string(), values.clone());
        let mut p = Product::new_configurable(
            ProductId::from_uuid(Uuid::from_u128(100)),
            Handle::new("shirt").unwrap(),
            "Shirt".to_string(),
            vec![option],
            NewVariant {
                id: var(100_000),
                sku: None,
                price: price(1),
                selections: vec![values[0].id()],
            },
        )
        .unwrap();
        for (n, value) in values
            .iter()
            .enumerate()
            .take(Product::MAX_VARIANTS)
            .skip(1)
        {
            p.add_variant(NewVariant {
                id: var(100_000 + n as u128),
                sku: None,
                price: price(1),
                selections: vec![value.id()],
            })
            .unwrap();
        }
        let one_more = NewVariant {
            id: var(200_000),
            sku: None,
            price: price(1),
            selections: vec![values[Product::MAX_VARIANTS].id()],
        };
        assert_eq!(
            p.add_variant(one_more),
            Err(ProductError::TooManyVariants {
                max: Product::MAX_VARIANTS
            })
        );
    }

    // ---- media ----

    #[test]
    fn media_keeps_its_order_and_the_first_is_featured() {
        let mut p = shirt();
        assert_eq!(p.set_media(vec![asset(2), asset(1)]), Ok(()));
        assert_eq!(p.media(), &[asset(2), asset(1)]);
        assert_eq!(p.featured_media(), Some(asset(2)));
    }

    #[test]
    fn media_refuses_an_asset_twice() {
        let mut p = shirt();
        assert_eq!(
            p.set_media(vec![asset(1), asset(2), asset(1)]),
            Err(ProductError::DuplicateMedia(asset(1)))
        );
    }

    #[test]
    fn media_has_at_most_max_items() {
        let mut p = shirt();
        let full: Vec<AssetId> = (0..Product::MAX_MEDIA as u128).map(asset).collect();
        assert_eq!(p.set_media(full.clone()), Ok(()));
        let mut over = full;
        over.push(asset(10_000));
        assert_eq!(
            p.set_media(over),
            Err(ProductError::TooManyMedia {
                max: Product::MAX_MEDIA
            })
        );
    }

    #[test]
    fn a_variant_shows_an_item_of_the_list_only() {
        let mut p = shirt();
        p.set_media(vec![asset(1)]).unwrap();
        assert_eq!(p.set_variant_media(var(1), Some(asset(1))), Ok(()));
        assert_eq!(p.variant(var(1)).unwrap().media(), Some(asset(1)));
        assert_eq!(
            p.set_variant_media(var(2), Some(asset(2))),
            Err(ProductError::MediaNotInProduct(asset(2)))
        );
        assert_eq!(p.set_variant_media(var(1), None), Ok(()));
        assert_eq!(p.variant(var(1)).unwrap().media(), None);
    }

    #[test]
    fn removing_an_item_clears_the_variants_showing_it() {
        let mut p = shirt();
        p.set_media(vec![asset(1), asset(2)]).unwrap();
        p.set_variant_media(var(1), Some(asset(1))).unwrap();
        p.set_variant_media(var(2), Some(asset(2))).unwrap();
        assert_eq!(p.set_media(vec![asset(2)]), Ok(()));
        assert_eq!(p.variant(var(1)).unwrap().media(), None);
        assert_eq!(p.variant(var(2)).unwrap().media(), Some(asset(2)));
    }
}
