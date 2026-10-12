use std::{collections::HashSet, fmt};

use hypha_core::{Price, ProductId, VariantId};
use serde::{Deserialize, Serialize};

use crate::{
    error::ProductError,
    handle::Handle,
    option::ProductOption,
    variant::{NewVariant, Variant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    id: ProductId,
    handle: Handle,
    title: String,
    status: ProductStatus,
    options: Vec<ProductOption>,
    variants: Vec<Variant>,
}

impl Product {
    pub fn new_simple(
        id: ProductId,
        handle: Handle,
        title: String,
        variant_id: VariantId,
        price: Price,
    ) -> Self {
        Self {
            id,
            handle,
            title,
            status: ProductStatus::Draft,
            options: Vec::new(),
            variants: vec![Variant::new(variant_id, None, price, Vec::new())],
        }
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

        for option in &options {
            if option.values().is_empty() {
                return Err(ProductError::EmptyOption(option.id()));
            }
        }

        let mut seen_names = HashSet::new();
        for option in &options {
            if !seen_names.insert(option.name()) {
                return Err(ProductError::DuplicateOptionName(option.name().to_string()));
            }
        }

        let mut seen_value_ids = HashSet::new();
        for option in &options {
            for value in option.values() {
                if !seen_value_ids.insert(value.id()) {
                    return Err(ProductError::DuplicateOptionValue(value.id()));
                }
            }
        }

        let mut product = Self {
            id,
            handle,
            title,
            status: ProductStatus::Draft,
            options,
            variants: Vec::new(),
        };
        product.add_variant(first_variant)?;
        Ok(product)
    }

    pub fn add_variant(&mut self, new: NewVariant) -> Result<(), ProductError> {
        if self.variants.iter().any(|v| v.id() == new.id) {
            return Err(ProductError::DuplicateVariantId(new.id));
        }

        if new.selections.len() != self.options.len() {
            return Err(ProductError::SelectionCountMismatch {
                expected: self.options.len(),
                got: new.selections.len(),
            });
        }

        for (option, selection) in self.options.iter().zip(&new.selections) {
            if !option.has_value(*selection) {
                return Err(ProductError::UnknownOptionValue(*selection));
            }
        }

        if self
            .variants
            .iter()
            .any(|v| v.selections() == new.selections.as_slice())
        {
            return Err(ProductError::DuplicateCombination);
        }

        self.variants
            .push(Variant::new(new.id, new.sku, new.price, new.selections));
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::option::OptionValue;
    use hypha_core::{Currency, Money, OptionId, OptionValueId};
    use uuid::Uuid;

    #[test]
    fn status_uses_snake_case_in_json_and_display() {
        for status in [
            ProductStatus::Draft,
            ProductStatus::Active,
            ProductStatus::Archived,
        ] {
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                format!("\"{status}\"")
            );
        }
        assert_eq!(
            serde_json::from_str::<ProductStatus>("\"active\"").unwrap(),
            ProductStatus::Active
        );
        assert!(serde_json::from_str::<ProductStatus>("\"Active\"").is_err());
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
}
