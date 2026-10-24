//! Contract tests every repository adapter must pass.
//!
//! One function per repo trait runs all its scenarios, each against a fresh repo from
//! `make`. The in-memory repos in this crate run them, and so must every storage adapter
//! (Postgres), so command tests and production see the same behavior:
//!
//! ```ignore
//! pollster::block_on(contract::product_repo(async || InMemoryProductRepo::default()));
//! ```
//!
//! For a database, `make` should hand out an empty store each time (for example after
//! truncating the tables). The functions are plain `async`, so any runtime can drive them.
//!
//! This is test-support code: a broken expectation panics, like an `assert!` in a test.
//! Each scenario's name is printed to stderr first, so a failure shows which one broke.
//!
//! What they check, per repo: every field survives a round trip (this catches mapping
//! bugs in SQL adapters), versions (`insert` -> 1, `update` bumps, stale or missing ->
//! `StaleVersion`), every unique key in the crate's `repo::keys`, and the batched
//! collection methods. `batch` checks `HasBatch` across aggregates.

mod agents;
mod apps;
mod auth;
mod batch;
mod carts;
mod categories;
mod channels;
mod custom_fields;
mod customer_groups;
mod customers;
mod deletions;
mod endpoints;
mod extensions;
mod fulfillments;
mod idempotency;
mod inventory;
mod markets;
mod media;
mod notifications;
mod oauth;
mod orders;
mod outbox;
mod payments;
mod preview_tokens;
mod products;
mod search;
mod shipping;
mod spore_settings;
mod staff_batch;
mod stores;
mod tax;
mod usages;
mod users;

pub use agents::{agent_call_repo, agent_repo, proposal_repo};
pub use apps::{app_deliveries, app_repo};
pub use auth::{api_key_repo, identity_repo, password_reset_repo, session_repo};
pub use batch::batch;
pub use carts::cart_repo;
pub use categories::category_repo;
pub use channels::channel_repo;
pub use custom_fields::{custom_field_definitions, custom_field_filters, custom_field_values};
pub use customer_groups::customer_group_repo;
pub use customers::customer_repo;
pub use deletions::deletions;
pub use endpoints::extension_endpoint_repo;
pub use extensions::extension_call_repo;
pub use fulfillments::fulfillment_repo;
pub use idempotency::idempotency_repo;
pub use inventory::{inventory_item_repo, location_repo};
pub use markets::{
    catalog_repo, market_repo, price_list_joins, price_list_repo, price_list_schedule,
};
pub use media::{asset_repo, blob_store};
pub use notifications::{
    notification_repo, notification_settings_batch, notification_settings_repo,
    notification_template_repo,
};
pub use oauth::oauth_repos;
pub use orders::order_repo;
pub use outbox::outbox;
pub use payments::{payment_batch, payment_method_delete, payment_method_repo, payment_repo};
pub use preview_tokens::preview_token_repo;
pub use products::{product_list, product_repo};
pub use search::{search_index, search_queue, search_settings_repo};
pub use shipping::{app_provider_repo, shipping_zone_repo};
pub use spore_settings::spore_settings_repo;
pub use staff_batch::staff_batch;
pub use stores::store_repo;
pub use tax::{tax_class_repo, tax_region_repo};
pub use usages::asset_usages;
pub use users::{invite_repo, role_repo, staff_sessions, user_repo};

fn scenario(name: &str) {
    eprintln!("contract scenario: {name}");
}
