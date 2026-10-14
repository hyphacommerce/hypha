//! HTTP routes. Each module owns its request/response shapes (the API's own types,
//! separate from domain types) and thin handlers that call one domain command.

mod carts;
mod customers;
mod health;
mod inventory;
mod markets;
mod orders;
mod products;
mod stores;
mod taxes;

use axum::{
    Router,
    routing::{delete, get, patch, post, put},
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
            .route("/health", get(health::check::<$storage>))
            // the store this deployment serves
            .route("/store", get(stores::fetch::<$storage>))
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
                "/markets/{id}/countries",
                post(markets::add_country_handler::<$storage>),
            )
            .route(
                "/markets/{id}/countries/{country}",
                delete(markets::remove_country_handler::<$storage>),
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
            .route(
                "/carts/{id}/lines/{variant}",
                put(carts::set_quantity::<$storage>).delete(carts::remove_line::<$storage>),
            )
            .route("/carts/{id}/email", put(carts::set_email::<$storage>))
            .route("/carts/{id}/customer", put(carts::set_customer::<$storage>))
            .route(
                "/carts/{id}/checkout",
                post(orders::checkout_handler::<$storage>),
            )
            // orders
            .route("/orders/{id}", get(orders::fetch::<$storage>))
            .route("/orders/{id}/paid", post(orders::mark_paid::<$storage>))
            // customers
            .route("/customers", post(customers::create::<$storage>))
            .route(
                "/customers/{id}",
                get(customers::fetch::<$storage>).patch(customers::update::<$storage>),
            )
            .route(
                "/customers/{id}/addresses",
                post(customers::add_address_handler::<$storage>),
            )
            .route(
                "/customers/{id}/addresses/{address_id}",
                put(customers::update_address_handler::<$storage>)
                    .delete(customers::remove_address_handler::<$storage>),
            )
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
            // inventory
            .route(
                "/locations",
                post(inventory::create_location_handler::<$storage>),
            )
            .route(
                "/locations/{id}",
                get(inventory::fetch_location::<$storage>)
                    .patch(inventory::update_location_handler::<$storage>),
            )
            .route("/inventory-items", post(inventory::create_item::<$storage>))
            .route(
                "/inventory-items/{id}",
                get(inventory::fetch_item::<$storage>).patch(inventory::update_item::<$storage>),
            )
            .route(
                "/inventory-items/{id}/levels/{location_id}",
                put(inventory::set_level::<$storage>),
            )
            .route(
                "/inventory-items/{id}/levels/{location_id}/adjustments",
                post(inventory::adjust_level::<$storage>),
            )
            .route(
                "/variants/{id}/inventory",
                get(inventory::fetch_variant_inventory::<$storage>),
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

#[cfg(test)]
mod tests;
