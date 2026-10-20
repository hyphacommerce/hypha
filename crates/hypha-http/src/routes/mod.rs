//! HTTP routes. Each module owns its request/response shapes (the API's own types,
//! separate from domain types) and thin handlers that call one domain command.

pub(crate) mod address;
// Handlers and their request and response types are `pub` so a host's router (`router!`,
// expanded in its crate) can name them; the types' fields stay private.
pub mod api_keys;
pub mod app_extensions;
pub mod apps;
pub mod assets;
pub mod auth;

pub use auth::password_reset_in_background;
pub mod carts;
pub mod categories;
pub mod channels;
pub mod custom_fields;
pub mod customers;
pub mod fulfillments;
pub mod health;
pub mod inventory;
pub mod markets;
pub mod me;
pub mod notifications;
pub mod oauth;
pub mod orders;
pub mod page;
pub mod payments;
pub mod products;
pub mod reference;
pub mod search;
pub mod setup;
pub mod shipping;
pub mod staff;
pub mod stores;
pub mod taxes;

use hypha_core::{Stamps, Timestamp};

use crate::error::ApiError;

/// A `Timestamp` as RFC 3339 in UTC, e.g. "2026-10-14T09:30:00.123456Z".
pub(crate) fn rfc3339(at: Timestamp) -> Result<String, ApiError> {
    jiff::Timestamp::from_microsecond(at.unix_micros())
        .map(|t| t.to_string())
        .map_err(ApiError::internal)
}

/// An aggregate's `created_at` and `updated_at` as RFC 3339 (design note 0016, Q3).
/// Responses show stored aggregates, which the adapter always stamps; one without
/// stamps is a bug, reported as a 500.
pub(crate) fn stamp_times(stamps: Option<Stamps>) -> Result<(String, String), ApiError> {
    let stamps = stamps.ok_or_else(|| ApiError::internal("an aggregate without stamps"))?;
    Ok((rfc3339(stamps.created_at)?, rfc3339(stamps.updated_at)?))
}

/// Every route, once (design note 0010, S7): `[surface, METHOD, "path", handler]`.
/// `$callback!` gets the whole table: `build_router!` turns it into a router per
/// storage, `openapi_rows!` (in `openapi.rs`) into the documents, so a route can't go
/// undocumented or a documented one unrouted.
///
/// Surfaces: `admin` (`/admin/*`, staff and secret keys), `storefront`
/// (`/storefront/*`, publishable keys), `hidden` (health and the documents themselves,
/// in no document).
#[doc(hidden)]
#[macro_export]
macro_rules! route_table {
    ([$($callback:tt)*] ( $($args:tt)* )) => {
        $($callback)*! { $($args)*
            [hidden, GET, "/health", health::check],
            [hidden, GET, "/admin/openapi.json", openapi::admin_document],
            [hidden, GET, "/storefront/openapi.json", openapi::storefront_document],
            // OAuth for apps (design note 0029): public, documented by their RFCs
            [hidden, GET, "/oauth/authorize", oauth::authorize_handler],
            [hidden, POST, "/oauth/token", oauth::token_handler],
            [hidden, POST, "/oauth/revoke", oauth::revoke_handler],
            [hidden, GET, "/.well-known/oauth-authorization-server", oauth::metadata],

            // ---------- admin ----------
            [admin, GET, "/admin/store", stores::fetch],
            [admin, PATCH, "/admin/store", stores::update],
            // reference data for pickers (0021)
            [admin, GET, "/admin/currencies", reference::currencies],
            [admin, GET, "/admin/countries", reference::countries],
            // staff auth (no credentials needed; rate limited, 0009)
            [admin, POST, "/admin/auth/login", auth::login_handler],
            [admin, POST, "/admin/auth/login/mfa", auth::finish_login_handler],
            [admin, POST, "/admin/auth/logout", auth::logout_handler],
            [admin, POST, "/admin/auth/password-reset", auth::request_reset_handler],
            [admin, POST, "/admin/auth/password-reset/confirm", auth::confirm_reset_handler],
            [admin, POST, "/admin/invites/accept", auth::accept_invite_handler],
            [admin, POST, "/admin/invites/lookup", auth::lookup_invite_handler],
            // the logged-in staff member
            [admin, GET, "/admin/me", me::fetch],
            [admin, PATCH, "/admin/me", me::update],
            [admin, GET, "/admin/me/sessions", me::list_sessions_handler],
            [admin, POST, "/admin/me/sessions/revoke-others", me::revoke_other_sessions_handler],
            [admin, DELETE, "/admin/me/sessions/{id}", me::revoke_session_handler],
            [admin, PUT, "/admin/me/password", me::change_password_handler],
            [admin, POST, "/admin/me/mfa/totp", me::start_totp_handler],
            [admin, POST, "/admin/me/mfa/totp/confirm", me::confirm_totp_handler],
            [admin, POST, "/admin/me/mfa/totp/disable", me::disable_totp_handler],
            [admin, POST, "/admin/me/mfa/recovery-codes", me::recovery_codes_handler],
            // staff, roles, invites, API keys
            [admin, GET, "/admin/roles", staff::list_roles_handler],
            [admin, POST, "/admin/roles", staff::create_role_handler],
            [admin, GET, "/admin/roles/{id}", staff::fetch_role],
            [admin, PATCH, "/admin/roles/{id}", staff::update_role_handler],
            [admin, GET, "/admin/users", staff::list_users_handler],
            [admin, GET, "/admin/users/{id}", staff::fetch_user],
            [admin, PATCH, "/admin/users/{id}", staff::update_user_handler],
            [admin, GET, "/admin/invites", staff::list_invites_handler],
            [admin, POST, "/admin/invites", staff::invite_handler],
            [admin, DELETE, "/admin/invites/{id}", staff::revoke_invite_handler],
            [admin, GET, "/admin/api-keys", api_keys::list],
            [admin, POST, "/admin/api-keys", api_keys::create],
            [admin, GET, "/admin/api-keys/{id}", api_keys::fetch],
            [admin, POST, "/admin/api-keys/{id}/revoke", api_keys::revoke],
            // products
            [admin, GET, "/admin/products", products::list],
            [admin, POST, "/admin/products", products::create],
            [admin, GET, "/admin/products/{id}", products::fetch],
            [admin, PATCH, "/admin/products/{id}", products::update],
            [admin, DELETE, "/admin/products/{id}", products::remove],
            [admin, PATCH, "/admin/products/{id}/status", products::set_status],
            [admin, PUT, "/admin/products/{id}/options", products::set_options],
            [admin, PUT, "/admin/products/{id}/variants", products::set_variants],
            [admin, POST, "/admin/products/{id}/variants", products::add_variant],
            [admin, PATCH, "/admin/products/{id}/variants/{variant_id}", products::update_variant],
            [admin, DELETE, "/admin/products/{id}/variants/{variant_id}", products::remove_variant],
            [admin, PUT, "/admin/products/{id}/media", products::set_media],
            [admin, PUT, "/admin/products/{id}/variants/{variant_id}/media", products::set_variant_media_handler],
            // media (0013)
            [admin, POST, "/admin/assets", assets::create],
            [admin, GET, "/admin/assets", assets::list],
            [admin, GET, "/admin/assets/{id}", assets::fetch],
            [admin, PATCH, "/admin/assets/{id}", assets::update],
            [admin, DELETE, "/admin/assets/{id}", assets::remove],
            [admin, POST, "/admin/assets/{id}/complete", assets::complete],
            [admin, GET, "/admin/assets/{id}/usages", assets::usages],
            // markets, price lists, catalogs
            [admin, GET, "/admin/markets", markets::list_markets_handler],
            [admin, POST, "/admin/markets", markets::create_market_handler],
            [admin, GET, "/admin/markets/{id}", markets::fetch_market],
            [admin, PATCH, "/admin/markets/{id}", markets::update_market_handler],
            [admin, POST, "/admin/markets/{id}/countries", markets::add_country_handler],
            [admin, DELETE, "/admin/markets/{id}/countries/{country}", markets::remove_country_handler],
            [admin, GET, "/admin/price-lists", markets::list_price_lists_handler],
            [admin, POST, "/admin/price-lists", markets::create_price_list_handler],
            [admin, GET, "/admin/price-lists/{id}", markets::fetch_price_list],
            [admin, PATCH, "/admin/price-lists/{id}", markets::update_price_list_handler],
            [admin, GET, "/admin/price-lists/{id}/fixed-prices", markets::list_fixed_prices_handler],
            [admin, PUT, "/admin/price-lists/{id}/fixed-prices", markets::set_fixed_prices_handler],
            [admin, DELETE, "/admin/price-lists/{id}/fixed-prices", markets::remove_fixed_prices_handler],
            [admin, POST, "/admin/catalogs", markets::create_catalog_handler],
            [admin, GET, "/admin/catalogs/{id}", markets::fetch_catalog],
            [admin, GET, "/admin/catalogs/{id}/products", markets::list_catalog_products_handler],
            [admin, POST, "/admin/catalogs/{id}/products", markets::add_products_handler],
            [admin, DELETE, "/admin/catalogs/{id}/products", markets::remove_products_handler],
            [admin, PUT, "/admin/catalogs/{id}/price-list", markets::assign_price_list_handler],
            // carts: only linking a customer (storefronts set an email; 0010 §2)
            [admin, PUT, "/admin/carts/{id}/customer", carts::set_customer],
            // orders
            [admin, GET, "/admin/orders", orders::list],
            [admin, GET, "/admin/orders/{id}", orders::fetch],
            [admin, POST, "/admin/orders/{id}/paid", orders::mark_paid],
            [admin, POST, "/admin/orders/{id}/cancel", orders::cancel],
            // payment methods (design note 0027)
            [admin, GET, "/admin/payment-providers", payments::list_providers],
            [admin, GET, "/admin/payment-methods", payments::list],
            [admin, POST, "/admin/payment-methods", payments::create],
            [admin, GET, "/admin/payment-methods/{id}", payments::fetch],
            [admin, PATCH, "/admin/payment-methods/{id}", payments::update],
            [admin, DELETE, "/admin/payment-methods/{id}", payments::delete],
            // an app's payments (design note 0029, §3.1)
            [admin, GET, "/admin/payments/{id}", payments::fetch_payment],
            [admin, POST, "/admin/payments/{id}/events", payments::report_event],
            [admin, POST, "/admin/payments/{id}/capture", payments::capture],
            [admin, POST, "/admin/payments/{id}/void", payments::void],
            [admin, POST, "/admin/payments/{id}/refund", payments::refund],
            // fulfilments (design note 0018)
            [admin, POST, "/admin/orders/{id}/fulfillments", fulfillments::create],
            [admin, GET, "/admin/orders/{id}/fulfillments", fulfillments::list],
            [admin, GET, "/admin/fulfillments/{id}", fulfillments::fetch],
            [admin, PATCH, "/admin/fulfillments/{id}", fulfillments::update],
            [admin, POST, "/admin/fulfillments/{id}/cancel", fulfillments::cancel],
            // customers
            [admin, GET, "/admin/customers", customers::list],
            [admin, POST, "/admin/customers", customers::create],
            [admin, GET, "/admin/customers/{id}", customers::fetch],
            [admin, PATCH, "/admin/customers/{id}", customers::update],
            [admin, POST, "/admin/customers/{id}/addresses", customers::add_address_handler],
            [admin, PUT, "/admin/customers/{id}/addresses/{address_id}", customers::update_address_handler],
            [admin, DELETE, "/admin/customers/{id}/addresses/{address_id}", customers::remove_address_handler],
            // taxes
            [admin, GET, "/admin/tax-classes", taxes::list_classes],
            [admin, POST, "/admin/tax-classes", taxes::create_class],
            [admin, PUT, "/admin/tax-assignments", taxes::assign_class],
            [admin, GET, "/admin/tax-regions", taxes::list_regions],
            [admin, POST, "/admin/tax-regions", taxes::create_region],
            [admin, GET, "/admin/tax-regions/{id}", taxes::fetch_region],
            [admin, PATCH, "/admin/tax-regions/{id}", taxes::update_region],
            [admin, PUT, "/admin/tax-regions/{id}/class-rates/{class_id}", taxes::set_region_class_rate],
            [admin, DELETE, "/admin/tax-regions/{id}/class-rates/{class_id}", taxes::remove_region_class_rate],
            // shipping (design note 0015)
            [admin, GET, "/admin/shipping-zones", shipping::list_zones_handler],
            [admin, POST, "/admin/shipping-zones", shipping::create_zone_handler],
            [admin, GET, "/admin/shipping-zones/{id}", shipping::fetch_zone],
            [admin, PATCH, "/admin/shipping-zones/{id}", shipping::update_zone_handler],
            [admin, DELETE, "/admin/shipping-zones/{id}", shipping::delete_zone_handler],
            [admin, POST, "/admin/shipping-zones/{id}/options", shipping::add_option_handler],
            [admin, PUT, "/admin/shipping-zones/{id}/options/order", shipping::reorder_options_handler],
            [admin, PATCH, "/admin/shipping-zones/{id}/options/{option_id}", shipping::update_option_handler],
            [admin, DELETE, "/admin/shipping-zones/{id}/options/{option_id}", shipping::remove_option_handler],
            [admin, GET, "/admin/shipping-providers", shipping::list_providers_handler],
            [admin, POST, "/admin/shipping-providers", shipping::create_provider_handler],
            [admin, GET, "/admin/shipping-providers/{key}", shipping::fetch_provider],
            [admin, PATCH, "/admin/shipping-providers/{key}", shipping::update_provider_handler],
            [admin, DELETE, "/admin/shipping-providers/{key}", shipping::delete_provider_handler],
            [admin, GET, "/admin/shipping-providers/{key}/services", shipping::provider_services_handler],
            // apps and webhooks (design note 0016)
            [admin, GET, "/admin/apps", apps::list],
            [admin, POST, "/admin/apps", apps::create],
            [admin, GET, "/admin/apps/{key}", apps::fetch],
            [admin, PATCH, "/admin/apps/{key}", apps::update],
            [admin, DELETE, "/admin/apps/{key}", apps::delete],
            [admin, POST, "/admin/apps/{key}/secret", apps::rotate_secret],
            [admin, GET, "/admin/apps/{key}/deliveries", apps::deliveries],
            [admin, GET, "/admin/apps/{key}/extension-calls", apps::extension_calls],
            [admin, GET, "/admin/apps/{key}/extension-points", apps::extension_points],
            [admin, PUT, "/admin/apps/{key}/extension-points", apps::set_extension_points],
            [admin, POST, "/admin/apps/{key}/deliveries/{id}/retry", apps::retry],
            // a spore's settings (design note 0030, §8)
            [admin, GET, "/admin/apps/{key}/settings", apps::settings],
            [admin, PUT, "/admin/apps/{key}/settings", apps::save_settings],
            [admin, GET, "/admin/events/{id}", apps::event],
            // installing apps (design note 0029)
            [admin, PATCH, "/admin/apps/{key}/grant", apps::narrow],
            [admin, GET, "/admin/oauth/requests/{id}", oauth::fetch_request],
            [admin, POST, "/admin/oauth/requests/{id}/approve", oauth::approve_request],
            [admin, POST, "/admin/oauth/requests/{id}/deny", oauth::deny_request],
            [admin, POST, "/admin/apps/private", oauth::create_private],
            [admin, POST, "/admin/apps/{key}/refresh", app_extensions::refresh],
            // app UI in any admin and storefront, the app proxy (design note 0029, §5-§8)
            [admin, GET, "/admin/app-extensions", app_extensions::list_admin],
            [admin, GET, "/admin/app-extensions/origins", app_extensions::origins],
            [admin, POST, "/admin/app-extensions/{app}/session", app_extensions::session],
            [admin, POST, "/admin/app-extensions/actions/{app}/{id}", app_extensions::run_action],
            [admin, GET, "/admin/app-extensions/blocks/{app}/{id}", app_extensions::fetch_block],
            [admin, GET, "/admin/apps/{key}/proxy/{*path}", app_extensions::admin_proxy_get],
            [admin, POST, "/admin/apps/{key}/proxy/{*path}", app_extensions::admin_proxy_post],
            [admin, PUT, "/admin/apps/{key}/proxy/{*path}", app_extensions::admin_proxy_put],
            [admin, PATCH, "/admin/apps/{key}/proxy/{*path}", app_extensions::admin_proxy_patch],
            [admin, DELETE, "/admin/apps/{key}/proxy/{*path}", app_extensions::admin_proxy_delete],
            // inventory
            [admin, GET, "/admin/locations", inventory::list_locations_handler],
            [admin, POST, "/admin/locations", inventory::create_location_handler],
            [admin, GET, "/admin/locations/{id}", inventory::fetch_location],
            [admin, PATCH, "/admin/locations/{id}", inventory::update_location_handler],
            [admin, POST, "/admin/inventory-items", inventory::create_item],
            [admin, GET, "/admin/inventory-items/{id}", inventory::fetch_item],
            [admin, PATCH, "/admin/inventory-items/{id}", inventory::update_item],
            [admin, PUT, "/admin/inventory-items/{id}/levels/{location_id}", inventory::set_level],
            [admin, POST, "/admin/inventory-items/{id}/levels/{location_id}/adjustments", inventory::adjust_level],
            [admin, GET, "/admin/variants/{id}/inventory", inventory::fetch_variant_inventory],
            [admin, GET, "/admin/products/{id}/inventory", inventory::fetch_product_inventory],
            [admin, GET, "/admin/setup/checklist", setup::checklist],

            // custom fields (design note 0033)
            [admin, GET, "/admin/custom-field-definitions", custom_fields::list],
            [admin, POST, "/admin/custom-field-definitions", custom_fields::create],
            [admin, GET, "/admin/custom-field-definitions/{id}", custom_fields::fetch],
            [admin, PATCH, "/admin/custom-field-definitions/{id}", custom_fields::update],
            [admin, DELETE, "/admin/custom-field-definitions/{id}", custom_fields::remove],
            [admin, PUT, "/admin/custom-field-definitions/{id}/position", custom_fields::move_to],
            [admin, GET, "/admin/custom-field-definitions/{id}/values-in-use", custom_fields::values],
            [admin, POST, "/admin/custom-fields/{definition}/items", custom_fields::change_items],
            [admin, POST, "/admin/custom-fields/{definition}/values", custom_fields::set_values],
            [admin, GET, "/admin/products/{id}/custom-fields", custom_fields::fetch_product],
            [admin, PATCH, "/admin/products/{id}/custom-fields", custom_fields::update_product],
            [admin, GET, "/admin/variants/{id}/custom-fields", custom_fields::fetch_variant],
            [admin, PATCH, "/admin/variants/{id}/custom-fields", custom_fields::update_variant],
            [admin, GET, "/admin/categories/{id}/custom-fields", custom_fields::fetch_category],
            [admin, PATCH, "/admin/categories/{id}/custom-fields", custom_fields::update_category],
            [admin, GET, "/admin/customers/{id}/custom-fields", custom_fields::fetch_customer],
            [admin, PATCH, "/admin/customers/{id}/custom-fields", custom_fields::update_customer],
            [admin, GET, "/admin/orders/{id}/custom-fields", custom_fields::fetch_order],
            [admin, PATCH, "/admin/orders/{id}/custom-fields", custom_fields::update_order],

            // categories (design note 0022)
            [admin, GET, "/admin/categories", categories::list],
            [admin, POST, "/admin/categories", categories::create],
            [admin, GET, "/admin/categories/{id}", categories::fetch],
            [admin, PATCH, "/admin/categories/{id}", categories::update],
            [admin, DELETE, "/admin/categories/{id}", categories::delete],
            [admin, GET, "/admin/categories/{id}/products", categories::list_products],
            [admin, POST, "/admin/categories/{id}/products", categories::add_products],
            [admin, DELETE, "/admin/categories/{id}/products", categories::remove_products],
            [admin, PUT, "/admin/categories/{id}/image", categories::set_image],
            [admin, PUT, "/admin/products/{id}/categories", categories::set_for_product],

            [admin, GET, "/admin/channels", channels::list],
            [admin, POST, "/admin/channels", channels::create],
            [admin, GET, "/admin/channels/{id}", channels::fetch],
            [admin, PATCH, "/admin/channels/{id}", channels::update],
            [admin, POST, "/admin/channels/{id}/products", channels::publish],
            [admin, DELETE, "/admin/channels/{id}/products", channels::unpublish],
            [admin, PUT, "/admin/products/{id}/channels", channels::set_for_product],
            [admin, GET, "/admin/search/settings", search::fetch_settings],
            [admin, PATCH, "/admin/search/settings", search::update_settings],
            [admin, GET, "/admin/search/status", search::status],
            [admin, POST, "/admin/search/reindex", search::start_reindex],
            // notifications (design note 0028)
            [admin, GET, "/admin/notification-settings", notifications::fetch_settings],
            [admin, PATCH, "/admin/notification-settings", notifications::update_settings],
            [admin, GET, "/admin/notification-templates", notifications::list_templates],
            [admin, GET, "/admin/notification-templates/{key}", notifications::fetch_template],
            [admin, PUT, "/admin/notification-templates/{key}", notifications::save_template],
            [admin, PATCH, "/admin/notification-templates/{key}", notifications::set_template_enabled],
            [admin, DELETE, "/admin/notification-templates/{key}", notifications::revert_template],
            [admin, POST, "/admin/notification-templates/{key}/preview", notifications::preview],
            [admin, POST, "/admin/notification-templates/{key}/test", notifications::send_test],
            [admin, GET, "/admin/notifications", notifications::list],
            [admin, GET, "/admin/notifications/{id}", notifications::fetch],
            [admin, POST, "/admin/notifications/{id}/resend", notifications::resend],

            // ---------- storefront ----------
            [storefront, GET, "/storefront/store", stores::fetch_storefront],
            [storefront, GET, "/storefront/products/{id}", products::fetch_storefront],
            [storefront, GET, "/storefront/products/by-handle/{handle}", products::fetch_by_handle_storefront],
            [storefront, GET, "/storefront/countries", markets::storefront_countries],
            [storefront, GET, "/storefront/categories", categories::list_storefront],
            [storefront, GET, "/storefront/categories/by-handle/{handle}", categories::fetch_by_handle_storefront],
            [storefront, GET, "/storefront/search", search::storefront_search],
            [storefront, POST, "/storefront/carts", carts::create],
            [storefront, GET, "/storefront/carts/{id}", carts::fetch],
            [storefront, POST, "/storefront/carts/{id}/lines", carts::add],
            [storefront, PUT, "/storefront/carts/{id}/lines/{variant}", carts::set_quantity],
            [storefront, DELETE, "/storefront/carts/{id}/lines/{variant}", carts::remove_line],
            [storefront, PUT, "/storefront/carts/{id}/email", carts::set_email],
            [storefront, PUT, "/storefront/carts/{id}/shipping-address", carts::set_address],
            [storefront, GET, "/storefront/carts/{id}/shipping-options", carts::shipping_options],
            [storefront, PUT, "/storefront/carts/{id}/shipping/{group_id}", carts::select],
            [storefront, GET, "/storefront/carts/{id}/payment-methods", carts::payment_methods],
            [storefront, PUT, "/storefront/carts/{id}/payment-method", carts::set_payment_method],
            [storefront, POST, "/storefront/carts/{id}/checkout", orders::checkout_handler],
            [storefront, POST, "/storefront/carts/{id}/payment", orders::start_cart_payment],
            [storefront, GET, "/storefront/orders/{id}", orders::fetch_placed],
            [storefront, GET, "/storefront/app-extensions", app_extensions::list_storefront],
            [storefront, GET, "/storefront/apps/{key}/proxy/{*path}", app_extensions::storefront_proxy_get],
            [storefront, POST, "/storefront/apps/{key}/proxy/{*path}", app_extensions::storefront_proxy_post],
            [storefront, PUT, "/storefront/apps/{key}/proxy/{*path}", app_extensions::storefront_proxy_put],
            [storefront, PATCH, "/storefront/apps/{key}/proxy/{*path}", app_extensions::storefront_proxy_patch],
            [storefront, DELETE, "/storefront/apps/{key}/proxy/{*path}", app_extensions::storefront_proxy_delete],
        }
    };
}

// After `route_table!`: a `macro_rules!` macro is only visible below its definition.
pub mod openapi;

/// The table's rows as a router for one concrete storage (`router!`'s callback).
#[doc(hidden)]
#[macro_export]
macro_rules! build_router {
    ($storage:ty $(, [$surface:ident, $method:ident, $path:literal, $module:ident::$func:ident])* $(,)?) => {
        $crate::routes::Router::new()
            $(.route(
                $path,
                $crate::routes::on(
                    $crate::routes::MethodFilter::$method,
                    $crate::routes::$module::$func::<$storage>,
                ),
            ))*
            .fallback(|| async { $crate::error::ApiError::route_not_found() })
            .method_not_allowed_fallback(|| async { $crate::error::ApiError::method_not_allowed() })
    };
}

/// The router for one concrete storage: `hypha_http::router!(PgStorage)` in a host. A
/// macro rather than a generic function: handlers are generic over the storage, but axum
/// must prove each handler's future is `Send`, and with our `Send`-free repo traits the
/// compiler can only do that for a concrete storage type.
#[macro_export]
macro_rules! router {
    ($storage:ty) => {
        $crate::route_table!([$crate::build_router]($storage,))
    };
}

#[doc(hidden)]
pub use axum::{
    Router,
    routing::{MethodFilter, on},
};
