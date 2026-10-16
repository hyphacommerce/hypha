pub mod commands;
pub mod content;
pub mod error;
pub mod events;
pub mod filter;
pub mod handle;
pub mod option;
pub mod product;
pub mod repo;
pub mod summary;
pub mod variant;

pub use commands::{
    AddVariant, CommandError, CreateSimpleProduct, ListProducts, SetProductOptions, SetVariant,
    UpdateProduct, UpdateVariant, add_product_variant, create_simple_product, get_product,
    get_products, list_products, products_of_variants, remove_product_variant, set_product_options,
    set_product_status, set_product_variants, update_product, update_product_variant,
};
pub use content::{Description, MAX_TAGS};
pub use error::{HandleError, ProductError};
pub use events::{ProductCreated, ProductUpdated, VariantAdded, VariantRemoved, VariantUpdated};
pub use filter::{ProductFilter, ProductQuery};
pub use handle::Handle;
pub use option::{OptionValue, ProductOption};
pub use product::{
    OptionDraft, OptionSnapshot, OptionValueDraft, OptionValueSnapshot, Product, ProductSnapshot,
    ProductStatus, VariantDraft, VariantSnapshot,
};
pub use repo::{HasProducts, ProductRepo, StageProducts};
pub use summary::{PriceRange, ProductSummary};
pub use variant::{NewVariant, Variant, VariantPatch};
