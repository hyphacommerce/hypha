use std::fmt::Display;

use hypha_cart::repo::keys as cart_keys;
use hypha_core::RepoError;
use hypha_customers::repo::keys as customer_keys;
use hypha_inventory::repo::keys as inventory_keys;
use hypha_markets::repo::keys as market_keys;
use hypha_products::repo::keys as product_keys;
use hypha_stores::repo::keys as store_keys;
use hypha_tax::repo::keys as tax_keys;
use tokio_postgres::error::SqlState;

/// Unique constraint names (see `migrations/`) and the repo key each one reports.
/// Postgres names the violated constraint, so this is an exact lookup, not text parsing.
const UNIQUE_KEYS: &[(&str, &str)] = &[
    ("products_pkey", product_keys::PRODUCT_ID),
    ("product_handle", product_keys::HANDLE),
    ("variant_id", product_keys::VARIANT_ID),
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
        // Two transactions locking the same rows in different orders (`update_many` on
        // overlapping items): Postgres aborts one. That is a concurrent change, safe to
        // retry like any other stale write.
        if *db.code() == SqlState::T_R_DEADLOCK_DETECTED {
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
