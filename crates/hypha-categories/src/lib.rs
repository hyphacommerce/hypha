//! Product categories (design note 0022): a plain tree of categories, products in any
//! number of them, and the rules of the tree (no cycles, a depth limit, unique handles).
//! A leaf crate on core: search reads membership through it, and `hypha-merchandising`
//! sets a category's image.

pub mod category;
pub mod commands;
pub mod error;
pub mod events;
pub mod repo;
pub mod settings;
pub mod tree;

pub use category::{Category, CategorySnapshot, NewCategory};
pub use commands::{
    CategoryPorts, CategoryView, CommandError, CreateCategory, UpdateCategory,
    add_category_products, categories_of_products, create_category, delete_category,
    find_categories, get_category, get_shown_category_by_handle, is_shown, list_categories,
    list_category_products, remove_category_products, set_product_categories, shown_categories,
    update_category,
};
pub use error::CategoryError;
pub use events::{
    CategoryChanged, CategoryDeleted, CategoryProductsAdded, CategoryProductsRemoved,
};
pub use repo::{CategoryRepo, HasCategories, StageCategories, keys};
pub use settings::CategorySettings;
pub use tree::CategoryTree;
