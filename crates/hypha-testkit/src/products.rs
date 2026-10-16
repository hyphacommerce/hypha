use hypha_core::{
    Page, PageRequest, ProductId, Recorded, RepoError, VariantId, Versioned, stamp_insert,
    stamp_update,
};
use hypha_products::{
    Handle, Product, ProductFilter, ProductSummary,
    repo::{ProductRepo, keys},
};

use crate::{InMemoryOutbox, table::Table};

#[derive(Default)]
pub struct InMemoryProductRepo {
    pub(crate) products: Table<ProductId, Product>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryProductRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            products: Table::default(),
            outbox: outbox.clone(),
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

/// The unique keys of `keys` that `new` would break against `other`.
pub(crate) fn unique(new: &Product, other: &Product) -> Option<&'static str> {
    if new.handle() == other.handle() {
        return Some(keys::HANDLE);
    }
    let shares_variant = new
        .variants()
        .iter()
        .any(|v| other.variants().iter().any(|o| o.id() == v.id()));
    if shares_variant {
        return Some(keys::VARIANT_ID);
    }
    let shares_sku = new
        .variants()
        .iter()
        .filter_map(|v| v.sku())
        .any(|sku| other.variants().iter().any(|o| o.sku() == Some(sku)));
    shares_sku.then_some(keys::VARIANT_SKU)
}

impl InMemoryProductRepo {
    /// The next `n` lookups by a unique key (and `list_all`, where there is one) find
    /// nothing, as if another request inserted the row right after: for testing a
    /// command that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.products.hide_next_lookups(n);
    }
}

impl ProductRepo for InMemoryProductRepo {
    async fn get(&self, id: ProductId) -> Result<Option<Product>, RepoError> {
        Ok(self.products.get(&id))
    }

    async fn get_many(&self, ids: &[ProductId]) -> Result<Vec<Product>, RepoError> {
        Ok(self.products.get_many(ids))
    }

    async fn list(
        &self,
        filter: &ProductFilter,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductSummary>, RepoError> {
        Ok(self
            .products
            .page(|p| filter.matches(p), page, Product::summary))
    }

    async fn get_by_handle(&self, handle: &Handle) -> Result<Option<Product>, RepoError> {
        Ok(self.products.find(|p| p.handle() == handle))
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<Product>, RepoError> {
        Ok(self
            .products
            .find(|p| p.variants().iter().any(|v| v.id() == variant)))
    }

    async fn get_by_sku(&self, sku: &str) -> Result<Option<Product>, RepoError> {
        Ok(self
            .products
            .find(|p| p.variants().iter().any(|v| v.sku() == Some(sku))))
    }

    async fn insert(&self, product: &mut Product) -> Result<(), RepoError> {
        stamp_insert(product, self.outbox.now());
        let events = product.take_events();
        self.products
            .insert(product.id(), product, keys::PRODUCT_ID, &unique)?;
        self.outbox.write(events, Some(product.version()));
        Ok(())
    }

    async fn update(&self, product: &mut Product) -> Result<(), RepoError> {
        stamp_update(product, self.outbox.now());
        let events = product.take_events();
        self.products.update(product.id(), product, &unique)?;
        self.outbox.write(events, Some(product.version()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, Money, Price, Version, Versioned};
    use hypha_products::ProductStatus;
    use pollster::block_on;
    use uuid::Uuid;

    use super::*;

    fn product(n: u128, handle: &str) -> Product {
        Product::new_simple(
            ProductId::from_uuid(Uuid::from_u128(n)),
            Handle::new(handle).unwrap(),
            "Test".to_string(),
            VariantId::from_uuid(Uuid::from_u128(n + 1000)),
            Price::new(Money::new(1000, Currency::Eur).unwrap()).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn insert_stores_product_at_version_1() {
        let repo = InMemoryProductRepo::default();
        let mut p = product(1, "mug");
        block_on(repo.insert(&mut p)).unwrap();
        assert_eq!(p.version(), Version::new(1));
        assert_eq!(block_on(repo.get(p.id())), Ok(Some(p)));
    }

    #[test]
    fn get_many_returns_only_existing_products() {
        let repo = InMemoryProductRepo::default();
        let mut mug = product(1, "mug");
        block_on(repo.insert(&mut mug)).unwrap();
        block_on(repo.insert(&mut product(2, "shirt"))).unwrap();
        let unknown = ProductId::from_uuid(Uuid::from_u128(99));

        assert_eq!(block_on(repo.get_many(&[mug.id(), unknown])), Ok(vec![mug]));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryProductRepo::default();
        let id = ProductId::from_uuid(Uuid::from_u128(99));
        assert_eq!(block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn get_by_handle_finds_the_right_product() {
        let repo = InMemoryProductRepo::default();
        let mut mug = product(1, "mug");
        block_on(repo.insert(&mut mug)).unwrap();
        block_on(repo.insert(&mut product(2, "shirt"))).unwrap();

        let found = block_on(repo.get_by_handle(&Handle::new("mug").unwrap()));
        assert_eq!(found, Ok(Some(mug)));

        let missing = block_on(repo.get_by_handle(&Handle::new("hat").unwrap()));
        assert_eq!(missing, Ok(None));
    }

    #[test]
    fn get_by_variant_finds_the_owning_product() {
        let repo = InMemoryProductRepo::default();
        let mut mug = product(1, "mug");
        block_on(repo.insert(&mut mug)).unwrap();
        block_on(repo.insert(&mut product(2, "shirt"))).unwrap();

        let variant = VariantId::from_uuid(Uuid::from_u128(1001));
        assert_eq!(block_on(repo.get_by_variant(variant)), Ok(Some(mug)));

        let unknown = VariantId::from_uuid(Uuid::from_u128(9999));
        assert_eq!(block_on(repo.get_by_variant(unknown)), Ok(None));
    }

    #[test]
    fn insert_rejects_duplicate_id_handle_and_variant() {
        let repo = InMemoryProductRepo::default();
        block_on(repo.insert(&mut product(1, "mug"))).unwrap();

        assert_eq!(
            block_on(repo.insert(&mut product(1, "other"))),
            Err(RepoError::Duplicate(keys::PRODUCT_ID))
        );
        assert_eq!(
            block_on(repo.insert(&mut product(2, "mug"))),
            Err(RepoError::Duplicate(keys::HANDLE))
        );
        // Product 3 reuses product 1's variant ID (1001).
        let reused_variant = Product::new_simple(
            ProductId::from_uuid(Uuid::from_u128(3)),
            Handle::new("cup").unwrap(),
            "Cup".to_string(),
            VariantId::from_uuid(Uuid::from_u128(1001)),
            Price::new(Money::new(1, Currency::Eur).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            block_on(repo.insert(&mut reused_variant.clone())),
            Err(RepoError::Duplicate(keys::VARIANT_ID))
        );
    }

    #[test]
    fn update_bumps_version() {
        let repo = InMemoryProductRepo::default();
        let mut p = product(1, "mug");
        block_on(repo.insert(&mut p)).unwrap();

        p.set_status(ProductStatus::Active).unwrap();
        block_on(repo.update(&mut p)).unwrap();

        assert_eq!(p.version(), Version::new(2));
        assert_eq!(block_on(repo.get(p.id())), Ok(Some(p)));
    }

    #[test]
    fn update_from_stale_copy_fails_and_keeps_newer_write() {
        let repo = InMemoryProductRepo::default();
        let mut p = product(1, "mug");
        block_on(repo.insert(&mut p)).unwrap();
        let mut first = p.clone();
        let mut second = p.clone();

        first.set_status(ProductStatus::Active).unwrap();
        block_on(repo.update(&mut first)).unwrap();
        second.set_status(ProductStatus::Active).unwrap();

        assert_eq!(
            block_on(repo.update(&mut second)),
            Err(RepoError::StaleVersion)
        );
        assert_eq!(block_on(repo.get(p.id())), Ok(Some(first)));
    }

    #[test]
    fn update_of_missing_product_is_stale() {
        let repo = InMemoryProductRepo::default();
        assert_eq!(
            block_on(repo.update(&mut product(1, "mug"))),
            Err(RepoError::StaleVersion)
        );
    }
}
