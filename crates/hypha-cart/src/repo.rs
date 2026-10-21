use hypha_core::{CartId, CustomerId, RepoError};

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
    /// Every cart linked to `customer`, open or completed, oldest first: what deleting
    /// or erasing the customer unlinks (design note 0032, §7.2).
    async fn of_customer(&self, customer: CustomerId) -> Result<Vec<Cart>, RepoError>;
}

/// Stages cart writes in a `hypha_core::HasBatch` batch; the adapter's batch type
/// implements it.
pub trait StageCarts {
    /// Stages what `CartRepo::insert` would write; `cart` is at version 1 once the batch
    /// commits. Enforces `CART_ID` on commit.
    fn insert_cart(&mut self, cart: &mut Cart);
    /// Stages what `CartRepo::update` would write, and moves `cart` to the version it has
    /// once the batch commits. If the commit fails, `cart` is stale: load it again.
    fn update_cart(&mut self, cart: &mut Cart);
    /// Removes the cart with its lines at the version it was loaded at (erasure deleting a
    /// customer's open carts, design note 0032, §7.3). The commit fails with
    /// `StaleVersion` if the stored cart has another version or is gone.
    fn delete_cart(&mut self, cart: &Cart);
}

/// Whatever holds the `CartRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use (review plan D1); `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasCarts {
    type Carts: CartRepo;
    fn carts(&self) -> &Self::Carts;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasCarts + ?Sized> HasCarts for &T {
    type Carts = T::Carts;
    fn carts(&self) -> &Self::Carts {
        (**self).carts()
    }
}
