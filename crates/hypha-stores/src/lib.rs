//! The store (merchant) a deployment serves, and its store-wide settings.
//!
//! Domain code never filters by store (design note 0006): the host resolves the store and
//! asks for ports scoped to it. This crate only describes the store itself.

pub mod commands;
pub mod error;
pub mod repo;
pub mod store;

pub use commands::{CommandError, create_store, ensure_single_store, get_store, rename_store};
pub use error::StoreError;
pub use repo::StoreRepo;
pub use store::{Store, StoreSnapshot};
