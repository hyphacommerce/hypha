pub mod commands;
pub mod error;
pub mod events;
pub mod handle;
pub mod option;
pub mod product;
pub mod repo;
pub mod summary;
pub mod variant;

pub use commands::{
    AddVariant, CommandError, CreateSimpleProduct, SetProductOptions, UpdateProduct, UpdateVariant,
    add_product_variant, create_simple_product, get_product, list_products, remove_product_variant,
    set_product_options, set_product_status, update_product, update_product_variant,
};
pub use error::{HandleError, ProductError};
pub use events::{ProductCreated, ProductUpdated, VariantAdded, VariantRemoved, VariantUpdated};
pub use handle::Handle;
pub use option::{OptionValue, ProductOption};
pub use product::{
    OptionDraft, OptionSnapshot, OptionValueDraft, OptionValueSnapshot, Product, ProductSnapshot,
    ProductStatus, VariantSnapshot,
};
pub use repo::{HasProducts, ProductRepo, StageProducts};
pub use summary::{PriceRange, ProductSummary};
pub use variant::{NewVariant, Variant, VariantPatch};
