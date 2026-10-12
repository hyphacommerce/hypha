use hypha_core::{CartId, RepoError};

use crate::cart::Cart;

/// Uniqueness rules a `CartRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const CART_ID: &str = "cart_id";
}

#[allow(async_fn_in_trait)]
pub trait CartRepo {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError>;
    /// Stores a new cart (version 1). Enforces `CART_ID`.
    async fn insert(&self, cart: &mut Cart) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion` if someone
    /// else changed the cart since it was loaded (two `add_line` calls at once).
    async fn update(&self, cart: &mut Cart) -> Result<(), RepoError>;
}
