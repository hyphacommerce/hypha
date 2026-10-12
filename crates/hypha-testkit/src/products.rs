use std::{collections::HashMap, sync::Mutex};

use hypha_core::{ProductId, RepoError, VariantId};
use hypha_products::{Handle, Product, repo::ProductRepo};

#[derive(Default)]
pub struct InMemoryProductRepo {
    products: Mutex<HashMap<ProductId, Product>>,
}

impl ProductRepo for InMemoryProductRepo {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, hypha_core::RepoError> {
        let products = self.products.lock().unwrap();
        Ok(products.get(&id).cloned())
    }

    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError> {
        let products = self.products.lock().unwrap();
        Ok(products.values().find(|p| p.handle() == handle).cloned())
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError> {
        let products = self.products.lock().unwrap();
        Ok(products
            .values()
            .find(|p| p.variants().iter().any(|v| v.id() == variant))
            .cloned())
    }

    async fn save(&self, product: &Product) -> Result<(), RepoError> {
        let mut products = self.products.lock().unwrap();
        products.insert(product.id(), product.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypha_core::{Currency, Money, Price};
    use uuid::Uuid;

    fn product(n: u128, handle: &str) -> Product {
        Product::new_simple(
            ProductId::from_uuid(Uuid::from_u128(n)),
            Handle::new(handle).unwrap(),
            "Test".to_string(),
            VariantId::from_uuid(Uuid::from_u128(n + 1000)),
            Price::new(Money::new(1000, Currency::Eur)).unwrap(),
        )
    }

    #[test]
    fn saves_and_loads_product() {
        let repo = InMemoryProductRepo::default();
        let p = product(1, "mug");
        pollster::block_on(repo.save(&p)).unwrap();
        assert_eq!(pollster::block_on(repo.get(p.id())), Ok(Some(p)));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryProductRepo::default();
        let id = ProductId::from_uuid(Uuid::from_u128(99));
        assert_eq!(pollster::block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn get_by_handle_finds_the_right_product() {
        let repo = InMemoryProductRepo::default();
        let mug = product(1, "mug");
        pollster::block_on(repo.save(&mug)).unwrap();
        pollster::block_on(repo.save(&product(2, "shirt"))).unwrap();

        let found = pollster::block_on(repo.get_by_handle(&Handle::new("mug").unwrap()));
        assert_eq!(found, Ok(Some(mug)));

        let missing = pollster::block_on(repo.get_by_handle(&Handle::new("hat").unwrap()));
        assert_eq!(missing, Ok(None));
    }

    #[test]
    fn get_by_variant_finds_the_owning_product() {
        let repo = InMemoryProductRepo::default();
        let mug = product(1, "mug");
        pollster::block_on(repo.save(&mug)).unwrap();
        pollster::block_on(repo.save(&product(2, "shirt"))).unwrap();

        let variant = VariantId::from_uuid(Uuid::from_u128(1001));
        assert_eq!(
            pollster::block_on(repo.get_by_variant(variant)),
            Ok(Some(mug))
        );

        let unknown = VariantId::from_uuid(Uuid::from_u128(9999));
        assert_eq!(pollster::block_on(repo.get_by_variant(unknown)), Ok(None));
    }
}
