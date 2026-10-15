//! What happens to tax settings, as events (design note 0016, section 4): prices shown
//! with tax change when these do. Contract types: a reshaped field is a new `VERSION`.

use hypha_core::{Event, ProductId, TaxClassId, TaxRegionId};
use serde::Serialize;

pub const TAX_CLASS: &str = "tax_class";
pub const TAX_REGION: &str = "tax_region";
/// Assignments are about products: their subject is the product.
pub const PRODUCT: &str = "product";

/// `tax_class.changed`: created (classes have nothing else to change yet).
#[derive(Debug, Serialize)]
pub struct TaxClassChanged {
    pub tax_class: TaxClassId,
}

impl Event for TaxClassChanged {
    const TYPE: &'static str = "tax_class.changed";
    const VERSION: u16 = 1;
}

/// `tax_region.changed`: created, its default rate or a class rate changed.
#[derive(Debug, Serialize)]
pub struct TaxRegionChanged {
    pub tax_region: TaxRegionId,
}

impl Event for TaxRegionChanged {
    const TYPE: &'static str = "tax_region.changed";
    const VERSION: u16 = 1;
}

/// `product.tax_class_set`: the product's tax class, `null` for the default rate.
#[derive(Debug, Serialize)]
pub struct ProductTaxClassSet {
    pub product: ProductId,
    pub tax_class: Option<TaxClassId>,
}

impl Event for ProductTaxClassSet {
    const TYPE: &'static str = "product.tax_class_set";
    const VERSION: u16 = 1;
}
