use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_categories::{Category, CategoryRepo, keys};
use hypha_core::{
    CategoryId, FieldFilter, Page, PageRequest, ProductId, Recorded, RepoError, Versioned,
    stamp_insert, stamp_update,
};

use crate::{
    InMemoryOutbox,
    custom_fields::{SharedValues, fields_match, values_for},
    table::{Table, missing, page_of},
};

/// Membership: each category's products.
pub(crate) type Members = HashMap<CategoryId, HashSet<ProductId>>;

#[derive(Default)]
pub struct InMemoryCategoryRepo {
    pub(crate) categories: Table<CategoryId, Category>,
    /// Stored apart from the categories, like a separate table would be; shared with the
    /// product repo, whose list filters by category (design note 0024, AU5).
    pub(crate) members: Arc<Mutex<Members>>,
    pub(crate) outbox: InMemoryOutbox,
    field_values: SharedValues,
}

impl InMemoryCategoryRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The same repo, filtering its list by the custom field values `fields` holds
    /// (design note 0033, §7), as a database joins them.
    pub fn joining_fields(self, fields: &crate::InMemoryCustomFields) -> Self {
        Self {
            field_values: fields.shared_values(),
            ..self
        }
    }

    /// Membership, locked, for a batch (`InMemoryPorts::commit`).
    pub(crate) fn lock_members(&self) -> MutexGuard<'_, Members> {
        self.members.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Handles are unique store-wide (CA6).
pub(crate) fn unique(new: &Category, other: &Category) -> Option<&'static str> {
    (new.handle() == other.handle()).then_some(keys::CATEGORY_HANDLE)
}

/// What a database's foreign key on the parent enforces: a category's parent exists.
pub(crate) fn check_parent(
    rows: &HashMap<CategoryId, Category>,
    category: &Category,
) -> Result<(), RepoError> {
    match category.parent() {
        Some(parent) if !rows.contains_key(&parent) => Err(missing("parent category", parent)),
        _ => Ok(()),
    }
}

impl CategoryRepo for InMemoryCategoryRepo {
    async fn get(&self, id: CategoryId) -> Result<Option<Category>, RepoError> {
        Ok(self.categories.get(&id))
    }

    async fn get_by_handle(&self, handle: &str) -> Result<Option<Category>, RepoError> {
        Ok(self.categories.find(|c| c.handle().as_str() == handle))
    }

    async fn list_all(&self) -> Result<Vec<Category>, RepoError> {
        Ok(self.categories.all())
    }

    async fn list_with_fields(&self, fields: &[FieldFilter]) -> Result<Vec<Category>, RepoError> {
        let values = values_for(&self.field_values, fields);
        Ok(self
            .categories
            .all()
            .into_iter()
            .filter(|c| fields_match(values.as_ref(), c.id().as_uuid(), fields))
            .collect())
    }

    async fn insert(&self, category: &mut Category) -> Result<(), RepoError> {
        check_parent(&self.categories.lock(), category)?;
        stamp_insert(category, self.outbox.now());
        let events = category.take_events();
        self.categories
            .insert(category.id(), category, keys::CATEGORY_ID, &unique)?;
        self.outbox.write(events, Some(category.version()));
        Ok(())
    }

    async fn update(&self, category: &mut Category) -> Result<(), RepoError> {
        check_parent(&self.categories.lock(), category)?;
        stamp_update(category, self.outbox.now());
        let events = category.take_events();
        self.categories.update(category.id(), category, &unique)?;
        self.outbox.write(events, Some(category.version()));
        Ok(())
    }

    async fn add_products(
        &self,
        category: CategoryId,
        products: &[ProductId],
    ) -> Result<(), RepoError> {
        if self.categories.get(&category).is_none() {
            return Err(missing("category", category));
        }
        self.lock_members()
            .entry(category)
            .or_default()
            .extend(products.iter().copied());
        Ok(())
    }

    async fn product_page(
        &self,
        category: CategoryId,
        page: PageRequest<ProductId>,
    ) -> Result<Page<ProductId>, RepoError> {
        let members = self.lock_members();
        let found = page_of(
            members
                .get(&category)
                .into_iter()
                .flatten()
                .map(|p| (*p, ())),
            page,
        );
        Ok(Page {
            items: found.items.into_iter().map(|(p, ())| p).collect(),
            has_more: found.has_more,
        })
    }

    async fn products_in(&self, categories: &[CategoryId]) -> Result<Vec<ProductId>, RepoError> {
        let members = self.lock_members();
        let mut found: Vec<ProductId> = categories
            .iter()
            .filter_map(|c| members.get(c))
            .flatten()
            .copied()
            .collect();
        found.sort();
        found.dedup();
        Ok(found)
    }

    async fn categories_of(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, Vec<CategoryId>>, RepoError> {
        let mut found: HashMap<ProductId, Vec<CategoryId>> = HashMap::new();
        for (category, members) in self.lock_members().iter() {
            for product in products.iter().filter(|p| members.contains(p)) {
                found.entry(*product).or_default().push(*category);
            }
        }
        for categories in found.values_mut() {
            categories.sort();
            categories.dedup();
        }
        Ok(found)
    }
}
