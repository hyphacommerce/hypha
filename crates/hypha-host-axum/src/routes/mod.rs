//! HTTP routes. Each module owns its request/response shapes (the API's own types,
//! separate from domain types) and thin handlers that call one domain command.

mod carts;
mod markets;
mod products;
mod taxes;

use axum::{
    Router,
    routing::{get, patch, post, put},
};

use crate::{
    error::ApiError,
    state::{MemoryStorage, PgStorage, SharedState},
};

/// Every route, once. A macro rather than a generic function: handlers are generic over
/// the storage, but axum must prove each handler's future is `Send`, and with our
/// `Send`-free repo traits the compiler can only do that for a concrete storage type.
macro_rules! router {
    ($storage:ty) => {
        Router::new()
            // products
            .route("/products", post(products::create::<$storage>))
            .route("/products/{id}", get(products::fetch::<$storage>))
            .route(
                "/products/{id}/status",
                patch(products::set_status::<$storage>),
            )
            // markets, price lists, catalogs
            .route("/markets", post(markets::create_market_handler::<$storage>))
            .route(
                "/markets/{id}",
                patch(markets::update_market_handler::<$storage>),
            )
            .route(
                "/price-lists",
                post(markets::create_price_list_handler::<$storage>),
            )
            .route(
                "/price-lists/{id}/fixed-prices",
                put(markets::set_fixed_prices_handler::<$storage>)
                    .delete(markets::remove_fixed_prices_handler::<$storage>),
            )
            .route(
                "/catalogs",
                post(markets::create_catalog_handler::<$storage>),
            )
            .route(
                "/catalogs/{id}/products",
                post(markets::add_products_handler::<$storage>)
                    .delete(markets::remove_products_handler::<$storage>),
            )
            .route(
                "/catalogs/{id}/price-list",
                put(markets::assign_price_list_handler::<$storage>),
            )
            // carts
            .route("/carts", post(carts::create::<$storage>))
            .route("/carts/{id}", get(carts::fetch::<$storage>))
            .route("/carts/{id}/lines", post(carts::add::<$storage>))
            // taxes
            .route("/tax-classes", post(taxes::create_class::<$storage>))
            .route("/tax-assignments", put(taxes::assign_class::<$storage>))
            .route("/tax-regions", post(taxes::create_region::<$storage>))
            .route(
                "/tax-regions/{id}",
                get(taxes::fetch_region::<$storage>).patch(taxes::update_region::<$storage>),
            )
            .route(
                "/tax-regions/{id}/class-rates/{class_id}",
                put(taxes::set_region_class_rate::<$storage>)
                    .delete(taxes::remove_region_class_rate::<$storage>),
            )
            // everything else
            .fallback(|| async { ApiError::route_not_found() })
            .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
    };
}

pub fn memory_router() -> Router<SharedState<MemoryStorage>> {
    router!(MemoryStorage)
}

pub fn postgres_router() -> Router<SharedState<PgStorage>> {
    router!(PgStorage)
}
