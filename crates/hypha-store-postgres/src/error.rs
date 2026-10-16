use std::fmt::Display;

use hypha_apps::keys as app_keys;
use hypha_auth::keys as auth_keys;
use hypha_cart::repo::keys as cart_keys;
use hypha_core::{RepoError, idempotency::keys as idempotency_keys};
use hypha_customers::repo::keys as customer_keys;
use hypha_fulfillment::keys as fulfillment_keys;
use hypha_inventory::repo::keys as inventory_keys;
use hypha_markets::repo::keys as market_keys;
use hypha_media::repo::keys as media_keys;
use hypha_orders::repo::keys as order_keys;
use hypha_products::repo::keys as product_keys;
use hypha_search_index::keys as search_keys;
use hypha_shipping::keys as shipping_keys;
use hypha_stores::repo::keys as store_keys;
use hypha_tax::repo::keys as tax_keys;
use hypha_users::keys as user_keys;
use tokio_postgres::error::SqlState;

/// Foreign keys whose violation is a domain rule (see `migrations/`), and the repo key
/// each reports as `RepoError::Reference`, whichever side of the reference broke it.
const REFERENCE_KEYS: &[(&str, &str)] = &[
    ("asset_usage_asset", media_keys::USAGE_ASSET),
    ("shipping_provider_app", shipping_keys::PROVIDER_APP),
];

/// Unique constraint names (see `migrations/`) and the repo key each one reports.
/// Postgres names the violated constraint, so this is an exact lookup, not text parsing.
const UNIQUE_KEYS: &[(&str, &str)] = &[
    ("products_pkey", product_keys::PRODUCT_ID),
    ("product_handle", product_keys::HANDLE),
    ("variant_id", product_keys::VARIANT_ID),
    ("variant_sku", product_keys::VARIANT_SKU),
    ("shipping_zone_id", shipping_keys::ZONE_ID),
    ("shipping_zone_country", shipping_keys::ZONE_COUNTRY),
    ("shipping_provider_key", shipping_keys::PROVIDER_KEY),
    ("markets_pkey", market_keys::MARKET_ID),
    ("market_country", market_keys::COUNTRY),
    ("catalogs_pkey", market_keys::CATALOG_ID),
    ("catalog_context", market_keys::CATALOG_CONTEXT),
    ("price_lists_pkey", market_keys::PRICE_LIST_ID),
    ("carts_pkey", cart_keys::CART_ID),
    ("tax_classes_pkey", tax_keys::TAX_CLASS_ID),
    ("tax_regions_pkey", tax_keys::TAX_REGION_ID),
    ("tax_region_country", tax_keys::TAX_REGION_COUNTRY),
    ("locations_pkey", inventory_keys::LOCATION_ID),
    ("inventory_items_pkey", inventory_keys::INVENTORY_ITEM_ID),
    (
        "inventory_item_variant",
        inventory_keys::INVENTORY_ITEM_VARIANT,
    ),
    ("customers_pkey", customer_keys::CUSTOMER_ID),
    ("customer_email", customer_keys::CUSTOMER_EMAIL),
    ("stores_pkey", store_keys::STORE_ID),
    ("search_settings_pkey", search_keys::SETTINGS),
    ("idempotency_key", idempotency_keys::IDEMPOTENCY_KEY),
    ("orders_pkey", order_keys::ORDER_ID),
    ("order_number", order_keys::ORDER_NUMBER),
    ("order_cart", order_keys::ORDER_CART),
    ("order_lines_pkey", order_keys::ORDER_LINE_ID),
    ("fulfillments_pkey", fulfillment_keys::FULFILLMENT_ID),
    ("identities_pkey", auth_keys::IDENTITY_ID),
    ("identity_principal", auth_keys::IDENTITY_PRINCIPAL),
    ("identity_login", auth_keys::IDENTITY_LOGIN),
    ("sessions_pkey", auth_keys::SESSION_ID),
    ("session_token", auth_keys::SESSION_TOKEN),
    ("api_keys_pkey", auth_keys::API_KEY_ID),
    ("api_key_token", auth_keys::API_KEY_TOKEN),
    ("password_resets_pkey", auth_keys::PASSWORD_RESET_ID),
    ("password_reset_token", auth_keys::PASSWORD_RESET_TOKEN),
    ("users_pkey", user_keys::USER_ID),
    ("user_email", user_keys::USER_EMAIL),
    ("roles_pkey", user_keys::ROLE_ID),
    ("role_name", user_keys::ROLE_NAME),
    ("role_owner", user_keys::ROLE_OWNER),
    ("invites_pkey", user_keys::INVITE_ID),
    ("invite_email", user_keys::INVITE_EMAIL),
    ("invite_token", user_keys::INVITE_TOKEN),
    ("assets_pkey", media_keys::ASSET_ID),
    ("app_key", app_keys::APP_KEY),
];

/// Maps a driver error to the repo error the ports promise.
pub(crate) fn repo_error(e: tokio_postgres::Error) -> RepoError {
    if let Some(db) = e.as_db_error() {
        if *db.code() == SqlState::UNIQUE_VIOLATION {
            let key = db.constraint().and_then(|name| {
                UNIQUE_KEYS
                    .iter()
                    .find(|(constraint, _)| *constraint == name)
                    .map(|(_, key)| *key)
            });
            if let Some(key) = key {
                return RepoError::Duplicate(key);
            }
        }
        if *db.code() == SqlState::FOREIGN_KEY_VIOLATION {
            let key = db.constraint().and_then(|name| {
                REFERENCE_KEYS
                    .iter()
                    .find(|(constraint, _)| *constraint == name)
                    .map(|(_, key)| *key)
            });
            if let Some(key) = key {
                return RepoError::Reference(key);
            }
        }
        // Two transactions locking the same rows in different orders (`update_many` on
        // overlapping items): Postgres aborts one. That is a concurrent change, safe to
        // retry like any other stale write.
        // A REPEATABLE READ transaction that can't be serialized with a concurrent write
        // (the read-only loads in `store.rs`) is the same situation: retry on fresh data.
        if *db.code() == SqlState::T_R_DEADLOCK_DETECTED
            || *db.code() == SqlState::T_R_SERIALIZATION_FAILURE
        {
            return RepoError::StaleVersion;
        }
        // `tokio_postgres::Error` only displays "db error"; the server's message is in here.
        return RepoError::Backend(db.to_string());
    }
    RepoError::Backend(e.to_string())
}

/// For data that came back from the database but doesn't make sense (a bug or a
/// manual edit): reported as a backend failure, never turned into an invalid aggregate.
pub(crate) fn corrupt(what: impl Display) -> RepoError {
    RepoError::Backend(format!("invalid stored data: {what}"))
}
