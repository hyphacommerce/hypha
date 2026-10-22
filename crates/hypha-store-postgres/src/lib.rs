//! Postgres storage for Hypha, on `tokio-postgres`.
//!
//! One `PgStore` implements every repository port. It never opens connections or picks a
//! runtime: the host hands it a connected client, so the same code runs natively (axum on
//! a VPS, Lambda; pooled connections) and on Cloudflare Workers through Hyperdrive (one
//! connection per request; Hyperdrive pools).
//!
//! Rules, from the design note:
//! - queries go through `Queries` (`store.rs`): unprepared (`query_typed*`) unless the
//!   host's connection keeps a `StatementCache`, which only a host connected straight to
//!   Postgres should: named prepared statements don't survive transaction pooling
//!   (Hyperdrive, older PgBouncer);
//! - every write is one short transaction with no reads in it besides its own version
//!   check, so it holds a pooled connection only briefly;
//! - errors are mapped by SQLSTATE and constraint name (`error.rs`), never by message text.

mod agents;
mod apps;
mod auth;
mod batch;
mod carts;
mod categories;
mod channels;
mod columns;
mod config;
mod custom_fields;
mod customer_groups;
mod customers;
mod endpoints;
mod error;
mod extensions;
mod field_filters;
mod fulfillments;
mod idempotency;
mod inventory;
mod markets;
mod media;
mod migrate;
mod notifications;
mod oauth;
mod orders;
mod outbox;
mod payments;
mod products;
mod sales;
mod search;
mod shipping;
pub mod spore_check;
mod spore_settings;
mod store;
mod stores;
mod tax;
#[cfg(feature = "testing")]
pub mod testing;
mod users;

pub use batch::PgBatch;
pub use config::{DEFAULT_CONNECT_TIMEOUT, database_config};
pub use error::spore_repo_error;
pub use migrate::{CORE, MigrateError, Migration, MigrationSet, migrate};
pub use store::{
    Db, PREVIEW_LEFT_OPEN, PgConnection, PgPreview, PgStore, Queries, StatementCache, StoreConn,
};
