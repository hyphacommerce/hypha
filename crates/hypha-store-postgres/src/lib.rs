//! Postgres storage for Hypha, on `tokio-postgres` (design note 0001, step 5).
//!
//! One `PgStore` implements every repository port. It never opens connections or picks a
//! runtime: the host hands it a connected client, so the same code runs natively (axum on
//! a VPS, Lambda; pooled connections) and on Cloudflare Workers through Hyperdrive (one
//! connection per request; Hyperdrive pools).
//!
//! Rules, from the design note:
//! - only `query_typed*` and `batch_execute`: no named prepared statements, which
//!   Hyperdrive's transaction pooling may not support;
//! - every write is one short transaction with no reads in it besides its own version
//!   check, so it holds a pooled connection only briefly;
//! - errors are mapped by SQLSTATE and constraint name (`error.rs`), never by message text.

mod apps;
mod auth;
mod batch;
mod carts;
mod columns;
mod config;
mod customers;
mod error;
mod fulfillments;
mod idempotency;
mod inventory;
mod markets;
mod media;
mod migrate;
mod orders;
mod outbox;
mod products;
mod shipping;
mod store;
mod stores;
mod tax;
mod users;

pub use batch::PgBatch;
pub use config::{DEFAULT_CONNECT_TIMEOUT, database_config};
pub use migrate::{CORE, MigrateError, Migration, MigrationSet, migrate};
pub use store::{PgConnection, PgStore};
