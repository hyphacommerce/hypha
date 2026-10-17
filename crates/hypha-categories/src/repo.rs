use std::collections::HashMap;

use hypha_core::{CategoryId, Page, PageRequest, ProductId, RepoError};

use crate::Category;

/// Uniqueness rules a `CategoryRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const CATEGORY_ID: &str = "category_id";
    /// Handles are unique store-wide, so a category keeps its URL when it moves (CA6).
    pub const CATEGORY_HANDLE: &str = "category_handle";
}

/// Categories, plus the products in each (their membership).
///
/// Membership is read and written in batches, like publications. A storefront lists a
/// category's products through search (design note 0022, section 3), not here.
#[allow(async_fn_in_trait)]
pub trait CategoryRepo {
    async fn get(&self, id: CategoryId) -> Result<Option<Category>, RepoError>;
    async fn get_by_handle(&self, handle: &str) -> Result<Option<Category>, RepoError>;
    /// Every category, in no particular order: a store has tens or hundreds, and
    /// `CategoryTree` orders them.
    async fn list_all(&self) -> Result<Vec<Category>, RepoError>;
    /// Stores a new category (version 1). Enforces `keys`.
    async fn insert(&self, category: &mut Category) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Enforces
    /// `CATEGORY_HANDLE`.
    async fn update(&self, category: &mut Category) -> Result<(), RepoError>;

    /// Adds products to the category; ones already in it are ignored. Records no
    /// event: commands stage it (`StageCategories`) with `category.products_added`;
    /// this is for setting up data.
    ///
    /// Precondition: the category exists. Otherwise adapters fail rather than store
    /// orphaned membership.
    async fn add_products(
        &self,
        category: CategoryId,
        products: &[ProductId],
    ) -> Result<(), RepoError>;
    /// A page of the products directly in the category, newest first (0012).
    async fn product_page(
        &self,
        category: CategoryId,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductId>, RepoError>;
    /// Every product directly in any of `categories`, each once, in no particular order:
    /// what search rebuilds when a subtree moves or hides.
    async fn products_in(&self, categories: &[CategoryId]) -> Result<Vec<ProductId>, RepoError>;
    /// Each of `products`' categories, sorted; a product in none is absent.
    async fn categories_of(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, Vec<CategoryId>>, RepoError>;
}

/// Category writes as a batch stages them, with their events (`StageEvents` for
/// membership). Staging moves a category to the version it gets on commit.
pub trait StageCategories {
    /// A new category; the commit enforces `keys`.
    fn insert_category(&mut self, category: &mut Category);
    /// A changed category; the commit fails with `StaleVersion` from an old copy.
    fn update_category(&mut self, category: &mut Category);
    /// Removes the category and its membership (never its products). Precondition: it
    /// has no children (commands check).
    fn delete_category(&mut self, category: CategoryId);
    /// Same preconditions as `CategoryRepo::add_products`.
    fn add_category_products(&mut self, category: CategoryId, products: &[ProductId]);
    fn remove_category_products(&mut self, category: CategoryId, products: &[ProductId]);
}

pub trait HasCategories {
    type Categories: CategoryRepo;
    fn categories(&self) -> &Self::Categories;
}

impl<T: HasCategories + ?Sized> HasCategories for &T {
    type Categories = T::Categories;
    fn categories(&self) -> &Self::Categories {
        (**self).categories()
    }
}
