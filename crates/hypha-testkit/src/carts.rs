use hypha_cart::{Cart, CartRepo, repo::keys};
use hypha_core::{CartId, CustomerId, RepoError, stamp_insert, stamp_update};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

#[derive(Default)]
pub struct InMemoryCartRepo {
    pub(crate) carts: Table<CartId, Cart>,
    /// Shared with `InMemoryPorts`' other repos: the clock writes are stamped with.
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryCartRepo {
    /// A repo sharing `outbox`'s clock (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl CartRepo for InMemoryCartRepo {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError> {
        Ok(self.carts.get(&id))
    }

    async fn insert(&self, cart: &mut Cart) -> Result<(), RepoError> {
        stamp_insert(cart, self.outbox.now());
        self.carts
            .insert(cart.id(), cart, keys::CART_ID, &no_unique_keys)
    }

    async fn update(&self, cart: &mut Cart) -> Result<(), RepoError> {
        stamp_update(cart, self.outbox.now());
        self.carts.update(cart.id(), cart, &no_unique_keys)
    }

    async fn of_customer(&self, customer: CustomerId) -> Result<Vec<Cart>, RepoError> {
        Ok(self
            .carts
            .all_oldest_first()
            .into_iter()
            .filter(|c| c.customer() == Some(customer))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::Quantity;
    use hypha_core::{CountryCode, MarketId, ProductId, VariantId, Version, Versioned};
    use pollster::block_on;
    use uuid::Uuid;

    use super::*;

    fn cart() -> Cart {
        Cart::new(
            CartId::from_uuid(Uuid::from_u128(1)),
            MarketId::from_uuid(Uuid::from_u128(2)),
            CountryCode::new("HR").unwrap(),
            hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
        )
    }

    #[test]
    fn insert_stores_cart_at_version_1() {
        let repo = InMemoryCartRepo::default();
        let mut c = cart();
        block_on(repo.insert(&mut c)).unwrap();
        assert_eq!(c.version(), Version::new(1));
        assert_eq!(block_on(repo.get(c.id())), Ok(Some(c)));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryCartRepo::default();
        let id = CartId::from_uuid(Uuid::from_u128(99));
        assert_eq!(block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn insert_rejects_duplicate_id() {
        let repo = InMemoryCartRepo::default();
        block_on(repo.insert(&mut cart())).unwrap();
        assert_eq!(
            block_on(repo.insert(&mut cart())),
            Err(RepoError::Duplicate(keys::CART_ID))
        );
    }

    /// The lost-update case: two writers load the same cart,
    /// both add a line, and the second save must fail instead of dropping a line.
    #[test]
    fn concurrent_changes_cannot_lose_a_line() {
        let repo = InMemoryCartRepo::default();
        let mut stored = cart();
        block_on(repo.insert(&mut stored)).unwrap();
        let mut a = block_on(repo.get(stored.id())).unwrap().unwrap();
        let mut b = block_on(repo.get(stored.id())).unwrap().unwrap();
        let qty = Quantity::new(1).unwrap();
        let product = ProductId::from_uuid(Uuid::from_u128(3));

        a.add_line(product, VariantId::from_uuid(Uuid::from_u128(4)), qty)
            .unwrap();
        b.add_line(product, VariantId::from_uuid(Uuid::from_u128(5)), qty)
            .unwrap();
        block_on(repo.update(&mut a)).unwrap();

        assert_eq!(block_on(repo.update(&mut b)), Err(RepoError::StaleVersion));
        assert_eq!(block_on(repo.get(stored.id())), Ok(Some(a)));
    }
}
