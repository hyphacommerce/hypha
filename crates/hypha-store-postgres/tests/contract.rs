//! Runs the shared repository contract (`hypha_testkit::contract`) against a real
//! Postgres. Needs `DATABASE_URL`, e.g. the database from the repo's `compose.yaml`:
//!
//!   docker compose up -d
//!   DATABASE_URL=postgres://hypha:hypha@localhost:5432/hypha cargo test -p hypha-store-postgres
//!
//! Without `DATABASE_URL` the tests print a notice and pass, so `cargo test --workspace`
//! still works on machines without Docker.

use hypha_store_postgres::{CORE, PgStore, migrate};
use hypha_testkit::contract;
use tokio_postgres::{Client, NoTls};

fn database_url() -> Option<String> {
    let url = std::env::var("DATABASE_URL").ok();
    if url.is_none() {
        eprintln!("DATABASE_URL not set: skipping the Postgres contract tests");
    }
    url
}

/// A store on a fresh, fully migrated schema. Each test uses its own schema name, so
/// tests running in parallel never see each other's rows.
async fn fresh_store(url: &str, schema: &str) -> PgStore<Client> {
    fresh_store_then(url, schema, "").await
}

/// `fresh_store`, then `sql` (to undo what migrations seed, such as the Online Store
/// channel).
async fn fresh_store_then(url: &str, schema: &str, sql: &str) -> PgStore<Client> {
    let (mut client, connection) = hypha_store_postgres::database_config(url)
        .expect("DATABASE_URL")
        .connect(NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client, &[CORE]).await.expect("migrate");
    if !sql.is_empty() {
        client.batch_execute(sql).await.expect("after migrating");
    }
    PgStore::new(client, clock())
}

#[tokio::test]
async fn product_repo() {
    let Some(url) = database_url() else { return };
    contract::product_repo(async || fresh_store(&url, "contract_products").await).await;
}

#[tokio::test]
async fn market_repo() {
    let Some(url) = database_url() else { return };
    contract::market_repo(async || fresh_store(&url, "contract_markets").await).await;
}

#[tokio::test]
async fn catalog_repo() {
    let Some(url) = database_url() else { return };
    contract::catalog_repo(async || fresh_store(&url, "contract_catalogs").await).await;
}

#[tokio::test]
async fn price_list_repo() {
    let Some(url) = database_url() else { return };
    contract::price_list_repo(async || fresh_store(&url, "contract_price_lists").await).await;
}

#[tokio::test]
async fn cart_repo() {
    let Some(url) = database_url() else { return };
    contract::cart_repo(async || fresh_store(&url, "contract_carts").await).await;
}

#[tokio::test]
async fn tax_class_repo() {
    let Some(url) = database_url() else { return };
    contract::tax_class_repo(async || fresh_store(&url, "contract_tax_classes").await).await;
}

#[tokio::test]
async fn tax_region_repo() {
    let Some(url) = database_url() else { return };
    contract::tax_region_repo(async || fresh_store(&url, "contract_tax_regions").await).await;
}

#[tokio::test]
async fn location_repo() {
    let Some(url) = database_url() else { return };
    contract::location_repo(async || fresh_store(&url, "contract_locations").await).await;
}

#[tokio::test]
async fn inventory_item_repo() {
    let Some(url) = database_url() else { return };
    contract::inventory_item_repo(async || fresh_store(&url, "contract_inventory_items").await)
        .await;
}

#[tokio::test]
async fn customer_repo() {
    let Some(url) = database_url() else { return };
    contract::customer_repo(async || fresh_store(&url, "contract_customers").await).await;
}

#[tokio::test]
async fn store_repo() {
    let Some(url) = database_url() else { return };
    contract::store_repo(async || fresh_store(&url, "contract_stores").await).await;
}

#[tokio::test]
async fn batch() {
    let Some(url) = database_url() else { return };
    contract::batch(async || fresh_store(&url, "contract_batch").await).await;
}

#[tokio::test]
async fn idempotency_repo() {
    let Some(url) = database_url() else { return };
    contract::idempotency_repo(async || fresh_store(&url, "contract_idempotency").await).await;
}

#[tokio::test]
async fn extension_call_repo() {
    let Some(url) = database_url() else { return };
    contract::extension_call_repo(async || fresh_store(&url, "contract_extension_calls").await)
        .await;
}

#[tokio::test]
async fn extension_endpoint_repo() {
    let Some(url) = database_url() else { return };
    contract::extension_endpoint_repo(async || {
        fresh_store(&url, "contract_extension_endpoints").await
    })
    .await;
}

#[tokio::test]
async fn order_repo() {
    let Some(url) = database_url() else { return };
    contract::order_repo(async || fresh_store(&url, "contract_orders").await).await;
}

#[tokio::test]
async fn fulfillment_repo() {
    let Some(url) = database_url() else { return };
    contract::fulfillment_repo(async || fresh_store(&url, "contract_fulfillments").await).await;
}

#[tokio::test]
async fn payment_method_repo() {
    let Some(url) = database_url() else { return };
    contract::payment_method_repo(async || fresh_store(&url, "contract_payment_methods").await)
        .await;
}

#[tokio::test]
async fn payment_method_delete() {
    let Some(url) = database_url() else { return };
    contract::payment_method_delete(async || {
        fresh_store(&url, "contract_payment_method_delete").await
    })
    .await;
}

#[tokio::test]
async fn payment_repo() {
    let Some(url) = database_url() else { return };
    contract::payment_repo(async || fresh_store(&url, "contract_payments").await).await;
}

#[tokio::test]
async fn payment_batch() {
    let Some(url) = database_url() else { return };
    contract::payment_batch(async || fresh_store(&url, "contract_payment_batch").await).await;
}

#[tokio::test]
async fn identity_repo() {
    let Some(url) = database_url() else { return };
    contract::identity_repo(async || fresh_store(&url, "contract_identities").await).await;
}

#[tokio::test]
async fn session_repo() {
    let Some(url) = database_url() else { return };
    contract::session_repo(async || fresh_store(&url, "contract_sessions").await).await;
}

#[tokio::test]
async fn api_key_repo() {
    let Some(url) = database_url() else { return };
    contract::api_key_repo(async || fresh_store(&url, "contract_api_keys").await).await;
}

#[tokio::test]
async fn password_reset_repo() {
    let Some(url) = database_url() else { return };
    contract::password_reset_repo(async || fresh_store(&url, "contract_password_resets").await)
        .await;
}

#[tokio::test]
async fn user_repo() {
    let Some(url) = database_url() else { return };
    contract::user_repo(async || fresh_store(&url, "contract_users").await).await;
}

#[tokio::test]
async fn role_repo() {
    let Some(url) = database_url() else { return };
    contract::role_repo(async || fresh_store(&url, "contract_roles").await).await;
}

#[tokio::test]
async fn invite_repo() {
    let Some(url) = database_url() else { return };
    contract::invite_repo(async || fresh_store(&url, "contract_invites").await).await;
}

#[tokio::test]
async fn staff_sessions() {
    let Some(url) = database_url() else { return };
    contract::staff_sessions(async || fresh_store(&url, "contract_staff_sessions").await).await;
}

#[tokio::test]
async fn staff_batch() {
    let Some(url) = database_url() else { return };
    contract::staff_batch(async || fresh_store(&url, "contract_staff_batch").await).await;
}

#[tokio::test]
async fn asset_repo() {
    let Some(url) = database_url() else { return };
    contract::asset_repo(async || fresh_store(&url, "contract_assets").await).await;
}

#[tokio::test]
async fn asset_usages() {
    let Some(url) = database_url() else { return };
    contract::asset_usages(async || fresh_store(&url, "contract_asset_usages").await).await;
}

#[tokio::test]
async fn shipping_zone_repo() {
    let Some(url) = database_url() else { return };
    contract::shipping_zone_repo(async || fresh_store(&url, "contract_shipping_zones").await).await;
}

#[tokio::test]
async fn app_provider_repo() {
    let Some(url) = database_url() else { return };
    contract::app_provider_repo(async || fresh_store(&url, "contract_app_providers").await).await;
}

#[tokio::test]
async fn app_repo() {
    let Some(url) = database_url() else { return };
    contract::app_repo(async || fresh_store(&url, "contract_apps").await).await;
}

#[tokio::test]
async fn app_deliveries() {
    let Some(url) = database_url() else { return };
    contract::app_deliveries(async || fresh_store(&url, "contract_app_deliveries").await).await;
}

#[tokio::test]
async fn outbox() {
    let Some(url) = database_url() else { return };
    contract::outbox(async || fresh_store(&url, "contract_outbox").await).await;
}

/// Writes are stamped with a fixed time in these tests.
fn clock() -> std::sync::Arc<hypha_testkit::FixedClock> {
    std::sync::Arc::new(hypha_testkit::FixedClock::default())
}

#[tokio::test]
async fn search_index() {
    let Some(url) = database_url() else { return };
    contract::search_index(async || fresh_store(&url, "contract_search_index").await).await;
}

#[tokio::test]
async fn search_queue() {
    let Some(url) = database_url() else { return };
    contract::search_queue(async || fresh_store(&url, "contract_search_queue").await).await;
}

#[tokio::test]
async fn search_settings_repo() {
    let Some(url) = database_url() else { return };
    contract::search_settings_repo(async || fresh_store(&url, "contract_search_settings").await)
        .await;
}

#[tokio::test]
async fn channel_repo() {
    let Some(url) = database_url() else { return };
    // The migration's Online Store would be in every list: start without it.
    contract::channel_repo(async || {
        fresh_store_then(&url, "contract_channels", "DELETE FROM channels").await
    })
    .await;
}

#[tokio::test]
async fn product_list() {
    let Some(url) = database_url() else { return };
    contract::product_list(async || fresh_store(&url, "contract_product_list").await).await;
}

#[tokio::test]
async fn category_repo() {
    let Some(url) = database_url() else { return };
    contract::category_repo(async || fresh_store(&url, "contract_categories").await).await;
}

#[tokio::test]
async fn notification_template_repo() {
    let Some(url) = database_url() else { return };
    contract::notification_template_repo(async || {
        fresh_store(&url, "contract_notification_templates").await
    })
    .await;
}

#[tokio::test]
async fn notification_settings_repo() {
    let Some(url) = database_url() else { return };
    contract::notification_settings_repo(async || {
        fresh_store(&url, "contract_notification_settings").await
    })
    .await;
}

#[tokio::test]
async fn notification_repo() {
    let Some(url) = database_url() else { return };
    contract::notification_repo(async || fresh_store(&url, "contract_notifications").await).await;
}

#[tokio::test]
async fn notification_settings_batch() {
    let Some(url) = database_url() else { return };
    contract::notification_settings_batch(async || {
        fresh_store(&url, "contract_notification_settings_batch").await
    })
    .await;
}
