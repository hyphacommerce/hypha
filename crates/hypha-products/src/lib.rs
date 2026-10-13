pub mod commands;
pub mod error;
pub mod handle;
pub mod option;
pub mod product;
pub mod repo;
pub mod variant;

pub use commands::{
    CommandError, CreateSimpleProduct, create_simple_product, get_product, set_product_status,
};
pub use error::{HandleError, ProductError};
pub use handle::Handle;
pub use option::{OptionValue, ProductOption};
pub use product::{
    OptionSnapshot, OptionValueSnapshot, Product, ProductSnapshot, ProductStatus, VariantSnapshot,
};
pub use repo::{HasProducts, ProductRepo};
pub use variant::{NewVariant, Variant};
