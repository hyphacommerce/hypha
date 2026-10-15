//! HTTP routes. Each module owns its request/response shapes (the API's own types,
//! separate from domain types) and thin handlers that call one domain command.

mod api_keys;
mod auth;

pub(crate) use auth::password_reset_in_background;
mod carts;
mod customers;
mod health;
mod inventory;
mod markets;
mod me;
mod orders;
mod page;
mod products;
mod staff;
mod stores;
mod taxes;

use axum::{
    Router,
    routing::{MethodFilter, on},
};

use hypha_core::Timestamp;

use crate::{
    error::ApiError,
    state::{PgStorage, SharedState},
};

/// A `Timestamp` as RFC 3339 in UTC, e.g. "2026-10-14T09:30:00.123456Z".
pub(crate) fn rfc3339(at: Timestamp) -> Result<String, ApiError> {
    jiff::Timestamp::from_microsecond(at.unix_micros())
        .map(|t| t.to_string())
        .map_err(ApiError::internal)
}

/// Every route, once (design note 0010, S7): `[surface, METHOD, "path", handler]`.
/// `$callback!` gets the whole table: `build_router!` turns it into a router per
/// storage, `openapi_rows!` (in `openapi.rs`) into the documents, so a route can't go
/// undocumented or a documented one unrouted.
///
/// Surfaces: `admin` (`/admin/*`, staff and secret keys), `storefront`
/// (`/storefront/*`, publishable keys), `hidden` (health and the documents themselves,
/// in no document).
macro_rules! route_table {
    ($callback:ident ! ( $($args:tt)* )) => {
        $callback! { $($args)*
            [hidden, GET, "/health", health::check],
            [hidden, GET, "/admin/openapi.json", openapi::admin_document],
            [hidden, GET, "/storefront/openapi.json", openapi::storefront_document],

            // ---------- admin ----------
            [admin, GET, "/admin/store", stores::fetch],
            [admin, PATCH, "/admin/store", stores::update],
            // staff auth (no credentials needed; rate limited, 0009)
            [admin, POST, "/admin/auth/login", auth::login_handler],
            [admin, POST, "/admin/auth/login/mfa", auth::finish_login_handler],
            [admin, POST, "/admin/auth/logout", auth::logout_handler],
            [admin, POST, "/admin/auth/password-reset", auth::request_reset_handler],
            [admin, POST, "/admin/auth/password-reset/confirm", auth::confirm_reset_handler],
            [admin, POST, "/admin/invites/accept", auth::accept_invite_handler],
            // the logged-in staff member
            [admin, GET, "/admin/me", me::fetch],
            [admin, PUT, "/admin/me/password", me::change_password_handler],
            [admin, POST, "/admin/me/mfa/totp", me::start_totp_handler],
            [admin, POST, "/admin/me/mfa/totp/confirm", me::confirm_totp_handler],
            [admin, POST, "/admin/me/mfa/totp/disable", me::disable_totp_handler],
            [admin, POST, "/admin/me/mfa/recovery-codes", me::recovery_codes_handler],
            // staff, roles, invites, API keys
            [admin, POST, "/admin/roles", staff::create_role_handler],
            [admin, GET, "/admin/roles/{id}", staff::fetch_role],
            [admin, PATCH, "/admin/roles/{id}", staff::update_role_handler],
            [admin, GET, "/admin/users/{id}", staff::fetch_user],
            [admin, PATCH, "/admin/users/{id}", staff::update_user_handler],
            [admin, POST, "/admin/invites", staff::invite_handler],
            [admin, POST, "/admin/api-keys", api_keys::create],
            [admin, GET, "/admin/api-keys/{id}", api_keys::fetch],
            [admin, POST, "/admin/api-keys/{id}/revoke", api_keys::revoke],
            // products
            [admin, GET, "/admin/products", products::list],
            [admin, POST, "/admin/products", products::create],
            [admin, GET, "/admin/products/{id}", products::fetch],
            [admin, PATCH, "/admin/products/{id}/status", products::set_status],
            // markets, price lists, catalogs
            [admin, POST, "/admin/markets", markets::create_market_handler],
            [admin, PATCH, "/admin/markets/{id}", markets::update_market_handler],
            [admin, POST, "/admin/markets/{id}/countries", markets::add_country_handler],
            [admin, DELETE, "/admin/markets/{id}/countries/{country}", markets::remove_country_handler],
            [admin, POST, "/admin/price-lists", markets::create_price_list_handler],
            [admin, PUT, "/admin/price-lists/{id}/fixed-prices", markets::set_fixed_prices_handler],
            [admin, DELETE, "/admin/price-lists/{id}/fixed-prices", markets::remove_fixed_prices_handler],
            [admin, POST, "/admin/catalogs", markets::create_catalog_handler],
            [admin, POST, "/admin/catalogs/{id}/products", markets::add_products_handler],
            [admin, DELETE, "/admin/catalogs/{id}/products", markets::remove_products_handler],
            [admin, PUT, "/admin/catalogs/{id}/price-list", markets::assign_price_list_handler],
            // carts: only linking a customer (storefronts set an email; 0010 §2)
            [admin, PUT, "/admin/carts/{id}/customer", carts::set_customer],
            // orders
            [admin, GET, "/admin/orders", orders::list],
            [admin, GET, "/admin/orders/{id}", orders::fetch],
            [admin, POST, "/admin/orders/{id}/paid", orders::mark_paid],
            // customers
            [admin, POST, "/admin/customers", customers::create],
            [admin, GET, "/admin/customers/{id}", customers::fetch],
            [admin, PATCH, "/admin/customers/{id}", customers::update],
            [admin, POST, "/admin/customers/{id}/addresses", customers::add_address_handler],
            [admin, PUT, "/admin/customers/{id}/addresses/{address_id}", customers::update_address_handler],
            [admin, DELETE, "/admin/customers/{id}/addresses/{address_id}", customers::remove_address_handler],
            // taxes
            [admin, POST, "/admin/tax-classes", taxes::create_class],
            [admin, PUT, "/admin/tax-assignments", taxes::assign_class],
            [admin, POST, "/admin/tax-regions", taxes::create_region],
            [admin, GET, "/admin/tax-regions/{id}", taxes::fetch_region],
            [admin, PATCH, "/admin/tax-regions/{id}", taxes::update_region],
            [admin, PUT, "/admin/tax-regions/{id}/class-rates/{class_id}", taxes::set_region_class_rate],
            [admin, DELETE, "/admin/tax-regions/{id}/class-rates/{class_id}", taxes::remove_region_class_rate],
            // inventory
            [admin, POST, "/admin/locations", inventory::create_location_handler],
            [admin, GET, "/admin/locations/{id}", inventory::fetch_location],
            [admin, PATCH, "/admin/locations/{id}", inventory::update_location_handler],
            [admin, POST, "/admin/inventory-items", inventory::create_item],
            [admin, GET, "/admin/inventory-items/{id}", inventory::fetch_item],
            [admin, PATCH, "/admin/inventory-items/{id}", inventory::update_item],
            [admin, PUT, "/admin/inventory-items/{id}/levels/{location_id}", inventory::set_level],
            [admin, POST, "/admin/inventory-items/{id}/levels/{location_id}/adjustments", inventory::adjust_level],
            [admin, GET, "/admin/variants/{id}/inventory", inventory::fetch_variant_inventory],

            // ---------- storefront ----------
            [storefront, GET, "/storefront/store", stores::fetch_storefront],
            [storefront, GET, "/storefront/products/{id}", products::fetch_storefront],
            [storefront, POST, "/storefront/carts", carts::create],
            [storefront, GET, "/storefront/carts/{id}", carts::fetch],
            [storefront, POST, "/storefront/carts/{id}/lines", carts::add],
            [storefront, PUT, "/storefront/carts/{id}/lines/{variant}", carts::set_quantity],
            [storefront, DELETE, "/storefront/carts/{id}/lines/{variant}", carts::remove_line],
            [storefront, PUT, "/storefront/carts/{id}/email", carts::set_email],
            [storefront, POST, "/storefront/carts/{id}/checkout", orders::checkout_handler],
        }
    };
}

// After `route_table!`: a `macro_rules!` macro is only visible below its definition.
pub(crate) mod openapi;

/// Builds the router for one concrete storage from the route table. A macro rather than
/// a generic function: handlers are generic over the storage, but axum must prove each
/// handler's future is `Send`, and with our `Send`-free repo traits the compiler can
/// only do that for a concrete storage type.
macro_rules! build_router {
    ($storage:ty $(, [$surface:ident, $method:ident, $path:literal, $module:ident::$func:ident])* $(,)?) => {
        Router::new()
            $(.route($path, on(MethodFilter::$method, $module::$func::<$storage>)))*
            .fallback(|| async { ApiError::route_not_found() })
            .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
    };
}

pub fn postgres_router() -> Router<SharedState<PgStorage>> {
    route_table!(build_router!(PgStorage,))
}

#[cfg(test)]
mod tests;
