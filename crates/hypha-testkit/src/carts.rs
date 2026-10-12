use std::{collections::HashMap, sync::Mutex};

use hypha_cart::{Cart, CartRepo};
use hypha_core::{CartId, RepoError};

#[derive(Default)]
pub struct InMemoryCartRepo {
    carts: Mutex<HashMap<CartId, Cart>>,
}

impl CartRepo for InMemoryCartRepo {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError> {
        let carts = self.carts.lock().unwrap();
        Ok(carts.get(&id).cloned())
    }

    async fn save(&self, cart: &Cart) -> Result<(), RepoError> {
        let mut carts = self.carts.lock().unwrap();
        carts.insert(cart.id(), cart.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::MarketId;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn saves_and_loads_carts() {
        let repo = InMemoryCartRepo::default();
        let cart = Cart::new(
            CartId::from_uuid(Uuid::from_u128(1)),
            MarketId::from_uuid(Uuid::from_u128(2)),
        );
        pollster::block_on(repo.save(&cart)).unwrap();
        assert_eq!(pollster::block_on(repo.get(cart.id())), Ok(Some(cart)));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryCartRepo::default();
        let id = CartId::from_uuid(Uuid::from_u128(99));
        assert_eq!(pollster::block_on(repo.get(id)), Ok(None));
    }
}
