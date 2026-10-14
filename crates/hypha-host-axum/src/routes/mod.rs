//! HTTP routes. Each module owns its request/response shapes (the API's own types,
//! separate from domain types) and thin handlers that call one domain command.

mod api_keys;
mod auth;
mod carts;
mod customers;
mod health;
mod inventory;
mod markets;
mod me;
mod orders;
mod products;
mod staff;
mod stores;
mod taxes;

use axum::{
    Router,
    routing::{delete, get, patch, post, put},
};

use hypha_core::Timestamp;

use crate::{
    error::ApiError,
    state::{MemoryStorage, PgStorage, SharedState},
};

/// A `Timestamp` as RFC 3339 in UTC, e.g. "2026-10-14T09:30:00.123456Z".
pub(crate) fn rfc3339(at: Timestamp) -> Result<String, ApiError> {
    jiff::Timestamp::from_microsecond(at.unix_micros())
        .map(|t| t.to_string())
        .map_err(ApiError::internal)
}

/// Every route, once. A macro rather than a generic function: handlers are generic over
/// the storage, but axum must prove each handler's future is `Send`, and with our
/// `Send`-free repo traits the compiler can only do that for a concrete storage type.
macro_rules! router {
    ($storage:ty) => {
        Router::new()
            .route("/health", get(health::check::<$storage>))
            // the store this deployment serves
            .route("/store", get(stores::fetch::<$storage>))
            // logging in (no credentials needed)
            .route("/auth/login", post(auth::login_handler::<$storage>))
            .route(
                "/auth/login/mfa",
                post(auth::finish_login_handler::<$storage>),
            )
            .route("/auth/logout", post(auth::logout_handler::<$storage>))
            .route(
                "/auth/password-reset",
                post(auth::request_reset_handler::<$storage>),
            )
            .route(
                "/auth/password-reset/confirm",
                post(auth::confirm_reset_handler::<$storage>),
            )
            .route(
                "/invites/accept",
                post(auth::accept_invite_handler::<$storage>),
            )
            // the logged-in staff member
            .route("/me", get(me::fetch::<$storage>))
            .route("/me/password", put(me::change_password_handler::<$storage>))
            .route("/me/mfa/totp", post(me::start_totp_handler::<$storage>))
            .route(
                "/me/mfa/totp/confirm",
                post(me::confirm_totp_handler::<$storage>),
            )
            .route(
                "/me/mfa/totp/disable",
                post(me::disable_totp_handler::<$storage>),
            )
            .route(
                "/me/mfa/recovery-codes",
                post(me::recovery_codes_handler::<$storage>),
            )
            // staff, roles, invites, API keys
            .route("/roles", post(staff::create_role_handler::<$storage>))
            .route(
                "/roles/{id}",
                get(staff::fetch_role::<$storage>).patch(staff::update_role_handler::<$storage>),
            )
            .route(
                "/users/{id}",
                get(staff::fetch_user::<$storage>).patch(staff::update_user_handler::<$storage>),
            )
            .route("/invites", post(staff::invite_handler::<$storage>))
            .route("/api-keys", post(api_keys::create::<$storage>))
            .route("/api-keys/{id}", get(api_keys::fetch::<$storage>))
            .route("/api-keys/{id}/revoke", post(api_keys::revoke::<$storage>))
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
