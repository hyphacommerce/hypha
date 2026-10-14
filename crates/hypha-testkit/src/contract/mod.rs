//! Contract tests every repository adapter must pass (design note 0001, 3.6).
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

mod batch;
mod carts;
mod customers;
mod inventory;
mod markets;
mod products;
mod stores;
mod tax;

pub use batch::batch;
pub use carts::cart_repo;
pub use customers::customer_repo;
pub use inventory::{inventory_item_repo, location_repo};
pub use markets::{catalog_repo, market_repo, price_list_repo};
pub use products::product_repo;
pub use stores::store_repo;
pub use tax::{tax_class_repo, tax_region_repo};

fn scenario(name: &str) {
    eprintln!("contract scenario: {name}");
}
