//! Merchandising: how products are presented. A composing crate above `hypha-products` and
//! `hypha-media`, which stay independent leaves: products store asset IDs without knowing
//! assets, and media records usages without knowing products. Attaching media to products
//! needs both, so it lives here, and so does a category's image.

pub mod commands;
pub mod handlers;
pub mod view;

pub use commands::{
    CommandError, MerchandisingPorts, category_image, delete_product, get_product_view,
    get_product_view_by_handle, list_product_views, set_category_image, set_product_media,
    set_variant_media,
};
pub use handlers::CategoryImageCleanup;
pub use view::{ProductSummaryView, ProductView};
