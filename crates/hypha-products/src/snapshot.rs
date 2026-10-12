//! Plain-data forms of the products aggregate, for storage adapters
//! (design note 0001, 3.4).
//!
//! Adapters only ever read and write snapshots: `Product::snapshot` to save,
//! `Product::restore` to load. `restore` re-validates every invariant, so bad stored
//! data fails loudly instead of becoming an invalid `Product`.

use hypha_core::{Money, OptionId, OptionValueId, ProductId, VariantId, Version};

use crate::ProductStatus;

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
}
