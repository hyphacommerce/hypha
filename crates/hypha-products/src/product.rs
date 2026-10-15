use std::{collections::HashSet, fmt, str::FromStr};

use hypha_core::{
    AssetId, Money, OptionId, OptionValueId, Price, ProductId, VariantId, Version, Versioned,
    optional_text, required_text,
};

use crate::{
    error::ProductError,
    handle::Handle,
    option::{OptionValue, ProductOption},
    variant::{NewVariant, Variant, VariantPatch},
};

mod edit;

pub use edit::{OptionDraft, OptionValueDraft};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductStatus {
    Draft,
    Active,
    Archived,
}

impl ProductStatus {
    /// The same string used in JSON, e.g. "active".
    pub fn as_str(&self) -> &'static str {
        match self {
            ProductStatus::Draft => "draft",
            ProductStatus::Active => "active",
            ProductStatus::Archived => "archived",
        }
    }
}

impl fmt::Display for ProductStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProductStatus {
    /// The text that didn't match a status.
    type Err = String;

    /// Parses the string produced by `as_str` (e.g. "active").
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [
            ProductStatus::Draft,
            ProductStatus::Active,
            ProductStatus::Archived,
        ]
        .into_iter()
        .find(|status| status.as_str() == s)
        .ok_or_else(|| s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    id: ProductId,
    version: Version,
    handle: Handle,
    title: String,
    status: ProductStatus,
    options: Vec<ProductOption>,
    variants: Vec<Variant>,
    /// Ordered; the first is the featured item. Asset IDs only: products don't know
    /// assets (design note 0014, E8).
    media: Vec<AssetId>,
}

impl Versioned for Product {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Product {
    /// Keeps a product one load, however large the catalog (design note 0014, Q5).
    pub const MAX_VARIANTS: usize = 2048;
    /// Shopify's limit (design note 0013, section 5).
    pub const MAX_MEDIA: usize = 250;

    pub fn new_simple(
        id: ProductId,
        handle: Handle,
        title: String,
        variant_id: VariantId,
        price: Price,
    ) -> Result<Self, ProductError> {
        Ok(Self {
            id,
            version: Version::NEW,
            handle,
            title: required_text("title", &title)?,
            status: ProductStatus::Draft,
            options: Vec::new(),
            variants: vec![Variant::new(variant_id, None, price, Vec::new())],
            media: Vec::new(),
        })
    }

    pub fn new_configurable(
        id: ProductId,
        handle: Handle,
        title: String,
        options: Vec<ProductOption>,
        first_variant: NewVariant,
    ) -> Result<Self, ProductError> {
        if options.is_empty() {
            return Err(ProductError::NoOptions);
        }
        validate_options(&options)?;

        let mut product = Self {
            id,
            version: Version::NEW,
            handle,
            title: required_text("title", &title)?,
            status: ProductStatus::Draft,
            options,
            variants: Vec::new(),
            media: Vec::new(),
        };
        product.add_variant(first_variant)?;
        Ok(product)
    }

    pub fn add_variant(&mut self, new: NewVariant) -> Result<(), ProductError> {
        if self.variants.iter().any(|v| v.id() == new.id) {
            return Err(ProductError::DuplicateVariantId(new.id));
        }
        self.check_selections(&new.selections, None)?;
        let sku = optional_text("sku", new.sku)?;
        self.check_sku(sku.as_deref(), None)?;
        if self.variants.len() >= Self::MAX_VARIANTS {
            return Err(ProductError::TooManyVariants {
                max: Self::MAX_VARIANTS,
            });
        }

        self.variants
            .push(Variant::new(new.id, sku, new.price, new.selections));
        Ok(())
    }

    /// `selections` name one value of each option, in option order, and no variant but
    /// `except` has the same combination.
    fn check_selections(
        &self,
        selections: &[OptionValueId],
        except: Option<VariantId>,
    ) -> Result<(), ProductError> {
        if selections.len() != self.options.len() {
            return Err(ProductError::SelectionCountMismatch {
                expected: self.options.len(),
                got: selections.len(),
            });
        }

        for (option, selection) in self.options.iter().zip(selections) {
            if !option.has_value(*selection) {
                return Err(ProductError::UnknownOptionValue(*selection));
            }
        }

        if self
            .variants
            .iter()
            .any(|v| Some(v.id()) != except && v.selections() == selections)
        {
            return Err(ProductError::DuplicateCombination);
        }
        Ok(())
    }

    /// No variant but `except` has `sku`. Other products are the repository's key
    /// (`keys::VARIANT_SKU`).
    fn check_sku(&self, sku: Option<&str>, except: Option<VariantId>) -> Result<(), ProductError> {
        match sku {
            Some(sku)
                if self
                    .variants
                    .iter()
                    .any(|v| Some(v.id()) != except && v.sku() == Some(sku)) =>
            {
                Err(ProductError::DuplicateSku(sku.to_string()))
            }
            _ => Ok(()),
        }
    }

    pub fn set_status(&mut self, to: ProductStatus) -> Result<(), ProductError> {
        if self.status == to {
            return Ok(());
        }

        match (self.status, to) {
            (ProductStatus::Draft, ProductStatus::Active)
            | (ProductStatus::Active, ProductStatus::Draft)
            | (ProductStatus::Active, ProductStatus::Archived)
            | (ProductStatus::Archived, ProductStatus::Draft) => {
                self.status = to;
                Ok(())
            }
            _ => Err(ProductError::InvalidTransition {
                from: self.status,
                to,
            }),
        }
    }
    pub fn id(&self) -> ProductId {
        self.id
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn status(&self) -> ProductStatus {
        self.status
    }

    pub fn options(&self) -> &[ProductOption] {
        &self.options
    }

    pub fn variants(&self) -> &[Variant] {
        &self.variants
    }

    pub fn variant(&self, id: VariantId) -> Option<&Variant> {
        self.variants.iter().find(|v| v.id() == id)
    }

    /// The media list, in order; the first is the featured item.
    pub fn media(&self) -> &[AssetId] {
        &self.media
    }

    pub fn featured_media(&self) -> Option<AssetId> {
        self.media.first().copied()
    }

    /// How `variant` is named next to the product title: its option values in option
    /// order, joined with " / " (`"M / Blue"`), as Shopify does. `None` for a simple
    /// product's variant, which has no options, and for a variant this product doesn't
    /// have.
    pub fn variant_title(&self, variant: VariantId) -> Option<String> {
        let variant = self.variants.iter().find(|v| v.id() == variant)?;
        if variant.selections().is_empty() {
            return None;
        }
        // Selections are in option order (`add_variant` checks it), so they pair up.
        let labels: Vec<&str> = self
            .options
            .iter()
            .zip(variant.selections())
            .filter_map(|(option, selected)| {
                option
                    .values()
                    .iter()
                    .find(|value| value.id() == *selected)
                    .map(|value| value.label())
            })
            .collect();
        Some(labels.join(" / "))
    }
}

/// Option rules shared by `new_configurable`, `set_options` and `restore`: names and
/// labels follow the shared text rule, every option has values, option names are unique
/// and so are labels within an option (both ignoring case), and value IDs are unique
/// across all options.
fn validate_options(options: &[ProductOption]) -> Result<(), ProductError> {
    for option in options {
        required_text("option name", option.name())?;
        if option.values().is_empty() {
            return Err(ProductError::EmptyOption(option.id()));
        }
        let mut seen_labels = HashSet::new();
        for value in option.values() {
            required_text("option value", value.label())?;
            if !seen_labels.insert(value.label().to_lowercase()) {
                return Err(ProductError::DuplicateOptionLabel {
                    option: option.name().to_string(),
                    label: value.label().to_string(),
                });
            }
        }
    }

    let mut seen_names = HashSet::new();
    for option in options {
        if !seen_names.insert(option.name().to_lowercase()) {
            return Err(ProductError::DuplicateOptionName(option.name().to_string()));
        }
    }

    let mut seen_value_ids = HashSet::new();
    for option in options {
        for value in option.values() {
            if !seen_value_ids.insert(value.id()) {
                return Err(ProductError::DuplicateOptionValue(value.id()));
            }
        }
    }
    Ok(())
}

/// Plain-data form of a `Product` for storage adapters (design note 0001, 3.4).
/// Adapters only ever read and write snapshots: `Product::snapshot` to save,
/// `Product::restore` to load. `restore` re-validates every invariant, so bad stored
/// data fails loudly instead of becoming an invalid `Product`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSnapshot {
    pub id: ProductId,
    pub version: Version,
    pub handle: String,
    pub title: String,
    pub status: ProductStatus,
    /// Empty for a simple product.
    pub options: Vec<OptionSnapshot>,
    /// In display order. A simple product has exactly one, with no selections.
    pub variants: Vec<VariantSnapshot>,
    /// In order; the first is featured.
    pub media: Vec<AssetId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionSnapshot {
    pub id: OptionId,
    pub name: String,
    pub values: Vec<OptionValueSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionValueSnapshot {
    pub id: OptionValueId,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSnapshot {
    pub id: VariantId,
    pub sku: Option<String>,
    pub base_price: Money,
    /// One value per option, in option order.
    pub selections: Vec<OptionValueId>,
    /// One of the product's `media`.
    pub media: Option<AssetId>,
    pub weight: Option<hypha_core::Weight>,
    pub requires_shipping: bool,
}

impl Product {
    /// Plain-data form for storage adapters.
    pub fn snapshot(&self) -> ProductSnapshot {
        ProductSnapshot {
            id: self.id,
            version: self.version,
            handle: self.handle.as_str().to_string(),
            title: self.title.clone(),
            status: self.status,
            options: self
                .options
                .iter()
                .map(|o| OptionSnapshot {
                    id: o.id(),
                    name: o.name().to_string(),
                    values: o
                        .values()
                        .iter()
                        .map(|v| OptionValueSnapshot {
                            id: v.id(),
                            label: v.label().to_string(),
                        })
                        .collect(),
                })
                .collect(),
            variants: self
                .variants
                .iter()
                .map(|v| VariantSnapshot {
                    id: v.id(),
                    sku: v.sku().map(str::to_string),
                    base_price: v.base_price().money(),
                    selections: v.selections().to_vec(),
                    media: v.media(),
                    weight: v.weight(),
                    requires_shipping: v.requires_shipping(),
                })
                .collect(),
            media: self.media.clone(),
        }
    }

    /// Rebuilds a product from storage, re-checking every invariant the constructors
    /// enforce. Any status is accepted: transitions were checked when it was set.
    pub fn restore(s: ProductSnapshot) -> Result<Product, ProductError> {
        let handle = Handle::new(&s.handle)?;
        let options: Vec<ProductOption> = s
            .options
            .into_iter()
            .map(|o| {
                let values = o
                    .values
                    .into_iter()
                    .map(|v| OptionValue::new(v.id, v.label))
                    .collect();
                ProductOption::new(o.id, o.name, values)
            })
            .collect();
        validate_options(&options)?;

        let mut product = Product {
            id: s.id,
            version: s.version,
            handle,
            title: required_text("title", &s.title)?,
            status: s.status,
            options,
            variants: Vec::with_capacity(s.variants.len()),
            media: Vec::new(),
        };
        product.set_media(s.media)?;
        // add_variant checks IDs, selection count, option values, unique combinations and
        // SKUs; with no options it allows exactly one variant (the simple-product shape).
        for v in s.variants {
            let price = Price::new(v.base_price).map_err(ProductError::InvalidPrice)?;
            product.add_variant(NewVariant {
                id: v.id,
                sku: v.sku,
                price,
                selections: v.selections,
            })?;
            product.set_variant_media(v.id, v.media)?;
            product.update_variant(
                v.id,
                VariantPatch {
                    weight: Some(v.weight),
                    requires_shipping: Some(v.requires_shipping),
                    ..VariantPatch::default()
                },
            )?;
        }
        if product.variants.is_empty() {
            return Err(ProductError::NoVariants);
        }
        Ok(product)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HandleError;
    use hypha_core::{Currency, Money, MoneyError, OptionId, OptionValueId};
    use uuid::Uuid;

    #[test]
    fn status_parses_its_own_strings_only() {
        for status in [
            ProductStatus::Draft,
            ProductStatus::Active,
            ProductStatus::Archived,
        ] {
            assert_eq!(status.as_str().parse::<ProductStatus>(), Ok(status));
        }
        assert_eq!("Active".parse::<ProductStatus>(), Err("Active".to_string()));
    }

    // ---- helpers ----

    fn val(n: u128) -> OptionValueId {
        OptionValueId::from_uuid(Uuid::from_u128(n))
    }

    fn var(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn opt(n: u128) -> OptionId {
        OptionId::from_uuid(Uuid::from_u128(n))
    }

    fn price() -> Price {
        Price::new(Money::new(1000, Currency::Eur).unwrap()).unwrap()
    }

    fn new_var(id: u128, selections: Vec<OptionValueId>) -> NewVariant {
        NewVariant {
            id: var(id),
            sku: None,
            price: price(),
            selections,
        }
    }

    // Size: S=11, M=12
    fn size() -> ProductOption {
        ProductOption::new(
            opt(1),
            "Size".to_string(),
            vec![
                OptionValue::new(val(11), "S".to_string()),
                OptionValue::new(val(12), "M".to_string()),
            ],
        )
    }

    // Color: Red=21, Blue=22
    fn color() -> ProductOption {
        ProductOption::new(
            opt(2),
            "Color".to_string(),
            vec![
                OptionValue::new(val(21), "Red".to_string()),
                OptionValue::new(val(22), "Blue".to_string()),
            ],
        )
    }

    fn configurable(
        options: Vec<ProductOption>,
        first: NewVariant,
    ) -> Result<Product, ProductError> {
        Product::new_configurable(
            ProductId::from_uuid(Uuid::from_u128(100)),
            Handle::new("shirt").unwrap(),
            "Shirt".to_string(),
            options,
            first,
        )
    }

    // Size + Color, first variant S + Red
    fn shirt() -> Product {
        configurable(vec![size(), color()], new_var(1, vec![val(11), val(21)])).unwrap()
    }

    fn simple() -> Product {
        Product::new_simple(
            ProductId::from_uuid(Uuid::from_u128(200)),
            Handle::new("mug").unwrap(),
            "Mug".to_string(),
            var(1),
            price(),
        )
        .unwrap()
    }

    // ---- new_simple ----

    #[test]
    fn new_simple_has_one_variant_no_options_and_is_draft() {
        let p = simple();
        assert_eq!(p.variants().len(), 1);
        assert!(p.options().is_empty());
        assert_eq!(p.status(), ProductStatus::Draft);
    }

    #[test]
    fn the_title_is_trimmed_and_required() {
        let titled = |title: &str| {
            Product::new_simple(
                ProductId::from_uuid(Uuid::from_u128(200)),
                Handle::new("mug").unwrap(),
                title.to_string(),
                var(1),
                price(),
            )
        };
        assert_eq!(
            titled(" Mug ").map(|p| p.title().to_string()),
            Ok("Mug".to_string())
        );
        assert_eq!(
            titled("  "),
            Err(ProductError::Text(hypha_core::TextError::Missing("title")))
        );
    }

    #[test]
    fn simple_product_rejects_second_variant() {
        let mut p = simple();
        let result = p.add_variant(new_var(2, vec![]));
        assert_eq!(result, Err(ProductError::DuplicateCombination));
    }

    // ---- new_configurable ----

    #[test]
    fn new_configurable_accepts_valid_options() {
        let p = shirt();
        assert_eq!(p.options().len(), 2);
        assert_eq!(p.variants().len(), 1);
        assert_eq!(p.status(), ProductStatus::Draft);
    }

    #[test]
    fn new_configurable_rejects_no_options() {
        let result = configurable(vec![], new_var(1, vec![]));
        assert_eq!(result, Err(ProductError::NoOptions));
    }

    #[test]
    fn new_configurable_rejects_empty_option() {
        let material = ProductOption::new(opt(3), "Material".to_string(), vec![]);
        let result = configurable(vec![size(), material], new_var(1, vec![val(11)]));
        assert_eq!(result, Err(ProductError::EmptyOption(opt(3))));
    }

    #[test]
    fn new_configurable_rejects_duplicate_option_name() {
        let second_size = ProductOption::new(
            opt(3),
            "Size".to_string(),
            vec![OptionValue::new(val(31), "L".to_string())],
        );
        let result = configurable(
            vec![size(), second_size],
            new_var(1, vec![val(11), val(31)]),
        );
        assert_eq!(
            result,
            Err(ProductError::DuplicateOptionName("Size".to_string()))
        );
    }

    #[test]
    fn new_configurable_rejects_duplicate_option_value() {
        // Color reuses S's value ID (11)
        let clashing_color = ProductOption::new(
            opt(2),
            "Color".to_string(),
            vec![OptionValue::new(val(11), "Red".to_string())],
        );
        let result = configurable(
            vec![size(), clashing_color],
            new_var(1, vec![val(11), val(11)]),
        );
        assert_eq!(result, Err(ProductError::DuplicateOptionValue(val(11))));
    }

    #[test]
    fn new_configurable_checks_first_variant() {
        let result = configurable(vec![size(), color()], new_var(1, vec![val(11)]));
        assert_eq!(
            result,
            Err(ProductError::SelectionCountMismatch {
                expected: 2,
                got: 1
            })
        );
    }

    // ---- add_variant ----

    #[test]
    fn accepts_new_combination() {
        let mut p = shirt();
        let result = p.add_variant(new_var(2, vec![val(12), val(22)])); // M + Blue
        assert_eq!(result, Ok(()));
        assert_eq!(p.variants().len(), 2);
    }

    #[test]
    fn rejects_duplicate_combination() {
        let mut p = shirt();
        let result = p.add_variant(new_var(2, vec![val(11), val(21)]));
        assert_eq!(result, Err(ProductError::DuplicateCombination));
    }

    #[test]
    fn rejects_wrong_selection_count() {
        let mut p = shirt();
        let result = p.add_variant(new_var(2, vec![val(12)]));
        assert_eq!(
            result,
            Err(ProductError::SelectionCountMismatch {
                expected: 2,
                got: 1
            })
        );
    }

    #[test]
    fn rejects_value_in_wrong_position() {
        let mut p = shirt();
        // Red in the Size position, M in the Color position
        let result = p.add_variant(new_var(2, vec![val(21), val(12)]));
        assert_eq!(result, Err(ProductError::UnknownOptionValue(val(21))));
    }

    #[test]
    fn rejects_duplicate_variant_id() {
        let mut p = shirt();
        let result = p.add_variant(new_var(1, vec![val(12), val(22)]));
        assert_eq!(result, Err(ProductError::DuplicateVariantId(var(1))));
    }

    // ---- variant_title ----

    #[test]
    fn variant_title_joins_option_values_in_option_order() {
        let mut p = shirt();
        p.add_variant(new_var(2, vec![val(12), val(22)])).unwrap();
        assert_eq!(p.variant_title(var(1)), Some("S / Red".to_string()));
        assert_eq!(p.variant_title(var(2)), Some("M / Blue".to_string()));
    }

    #[test]
    fn a_simple_products_variant_has_no_title() {
        assert_eq!(simple().variant_title(var(1)), None);
    }

    #[test]
    fn an_unknown_variant_has_no_title() {
        assert_eq!(shirt().variant_title(var(99)), None);
    }

    // ---- set_status ----

    fn with_status(status: ProductStatus) -> Product {
        let mut p = simple();
        p.status = status;
        p
    }

    #[test]
    fn allows_valid_transitions() {
        use ProductStatus::*;
        for (from, to) in [
            (Draft, Active),
            (Active, Draft),
            (Active, Archived),
            (Archived, Draft),
        ] {
            let mut p = with_status(from);
            assert_eq!(p.set_status(to), Ok(()), "{from:?} -> {to:?}");
            assert_eq!(p.status(), to, "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn setting_same_status_is_a_no_op() {
        let mut p = with_status(ProductStatus::Active);
        assert_eq!(p.set_status(ProductStatus::Active), Ok(()));
        assert_eq!(p.status(), ProductStatus::Active);
    }

    #[test]
    fn rejects_draft_to_archived() {
        let mut p = with_status(ProductStatus::Draft);
        assert_eq!(
            p.set_status(ProductStatus::Archived),
            Err(ProductError::InvalidTransition {
                from: ProductStatus::Draft,
                to: ProductStatus::Archived,
            })
        );
        assert_eq!(p.status(), ProductStatus::Draft);
    }

    #[test]
    fn rejects_archived_to_active() {
        let mut p = with_status(ProductStatus::Archived);
        assert_eq!(
            p.set_status(ProductStatus::Active),
            Err(ProductError::InvalidTransition {
                from: ProductStatus::Archived,
                to: ProductStatus::Active,
            })
        );
        assert_eq!(p.status(), ProductStatus::Archived);
    }
    // ---- snapshot / restore ----

    #[test]
    fn restore_round_trips_configurable_product() {
        let mut p = shirt();
        p.add_variant(new_var(2, vec![val(12), val(22)])).unwrap();
        p.set_status(ProductStatus::Active).unwrap();
        p.set_version(Version::new(7));
        assert_eq!(Product::restore(p.snapshot()), Ok(p));
    }

    #[test]
    fn restore_round_trips_shipping_facts() {
        let mut p = shirt();
        p.update_variant(
            var(1),
            VariantPatch {
                weight: Some(Some(hypha_core::Weight::from_grams(500))),
                requires_shipping: Some(false),
                ..VariantPatch::default()
            },
        )
        .unwrap();
        assert_eq!(Product::restore(p.snapshot()), Ok(p));
    }

    #[test]
    fn restore_round_trips_simple_product() {
        let p = simple();
        assert_eq!(Product::restore(p.snapshot()), Ok(p));
    }

    #[test]
    fn restore_accepts_any_stored_status() {
        // Draft -> Archived is not an allowed transition, but stored state is just state.
        let mut s = simple().snapshot();
        s.status = ProductStatus::Archived;
        assert_eq!(
            Product::restore(s).map(|p| p.status()),
            Ok(ProductStatus::Archived)
        );
    }

    #[test]
    fn restore_rejects_invalid_handle() {
        let mut s = simple().snapshot();
        s.handle = "Bad Handle".to_string();
        assert_eq!(
            Product::restore(s),
            Err(ProductError::InvalidHandle(HandleError::InvalidChar('B')))
        );
    }

    #[test]
    fn restore_rejects_negative_price() {
        let mut s = simple().snapshot();
        s.variants[0].base_price = Money::new(-1, Currency::Eur).unwrap();
        assert_eq!(
            Product::restore(s),
            Err(ProductError::InvalidPrice(MoneyError::Negative))
        );
    }

    #[test]
    fn restore_rejects_product_without_variants() {
        let mut s = simple().snapshot();
        s.variants.clear();
        assert_eq!(Product::restore(s), Err(ProductError::NoVariants));
    }

    #[test]
    fn restore_rejects_second_variant_on_simple_product() {
        let mut s = simple().snapshot();
        s.variants.push(VariantSnapshot {
            id: var(2),
            sku: None,
            base_price: price().money(),
            selections: vec![],
            media: None,
            weight: None,
            requires_shipping: true,
        });
        assert_eq!(Product::restore(s), Err(ProductError::DuplicateCombination));
    }

    #[test]
    fn restore_rejects_selection_of_unknown_value() {
        let mut s = shirt().snapshot();
        s.variants[0].selections = vec![val(11), val(99)];
        assert_eq!(
            Product::restore(s),
            Err(ProductError::UnknownOptionValue(val(99)))
        );
    }

    #[test]
    fn restore_rejects_duplicate_option_value() {
        let mut s = shirt().snapshot();
        s.options[1].values[0].id = val(11);
        assert_eq!(
            Product::restore(s),
            Err(ProductError::DuplicateOptionValue(val(11)))
        );
    }
}
