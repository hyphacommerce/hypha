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

mod batch;
mod carts;
mod columns;
mod customers;
mod error;
mod idempotency;
mod inventory;
mod markets;
mod migrate;
mod products;
mod store;
mod stores;
mod tax;

pub use batch::PgBatch;
pub use migrate::{CORE, MigrateError, Migration, MigrationSet, migrate};
pub use store::{PgConnection, PgStore};
