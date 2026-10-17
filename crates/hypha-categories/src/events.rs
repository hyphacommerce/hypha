//! What happens to categories, as events (design note 0022, section 2). Contract types:
//! a renamed or reshaped field is a new `VERSION`, never an edit.

use std::collections::BTreeSet;

use hypha_core::{CategoryId, Event, PendingEvent, ProductId, Subject};
use serde::Serialize;

pub const CATEGORY: &str = "category";

/// `category.changed`: created, or which of its fields changed, among `name`,
/// `handle`, `description`, `image`, `parent`, `position` and `visible`. Search
/// rebuilds the subtree's products when `parent` or `visible` changed.
#[derive(Debug, Serialize)]
pub struct CategoryChanged {
    pub category: CategoryId,
    pub created: bool,
    pub fields: Vec<&'static str>,
}

impl Event for CategoryChanged {
    const TYPE: &'static str = "category.changed";
    const VERSION: u16 = 1;
}

/// `category.deleted`, with the products that were in it (their memberships went with
/// it).
#[derive(Debug, Serialize)]
pub struct CategoryDeleted {
    pub category: CategoryId,
    pub products: Vec<ProductId>,
}

impl Event for CategoryDeleted {
    const TYPE: &'static str = "category.deleted";
    const VERSION: u16 = 1;
}

/// `category.products_added`: these products are in the category now.
#[derive(Debug, Serialize)]
pub struct CategoryProductsAdded {
    pub category: CategoryId,
    pub products: Vec<ProductId>,
}

impl Event for CategoryProductsAdded {
    const TYPE: &'static str = "category.products_added";
    const VERSION: u16 = 1;
}

/// `category.products_removed`: these products aren't in the category any more.
#[derive(Debug, Serialize)]
pub struct CategoryProductsRemoved {
    pub category: CategoryId,
    pub products: Vec<ProductId>,
}

impl Event for CategoryProductsRemoved {
    const TYPE: &'static str = "category.products_removed";
    const VERSION: u16 = 1;
}

/// What changed in a category since it was loaded, coalesced into one
/// `category.changed` per write. Compares as always equal, so it never makes two
/// categories unequal.
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    created: bool,
    fields: BTreeSet<&'static str>,
}

impl PartialEq for Changes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Changes {}

impl Changes {
    pub(crate) fn created() -> Self {
        Self {
            created: true,
            fields: BTreeSet::new(),
        }
    }

    pub(crate) fn field(&mut self, field: &'static str) {
        if !self.created {
            self.fields.insert(field);
        }
    }

    pub(crate) fn take(&mut self, category: CategoryId) -> Vec<PendingEvent> {
        let changes = std::mem::take(self);
        if !changes.created && changes.fields.is_empty() {
            return Vec::new();
        }
        let event = CategoryChanged {
            category,
            created: changes.created,
            fields: changes.fields.into_iter().collect(),
        };
        vec![PendingEvent::new(
            Subject::new(CATEGORY, category.as_uuid()),
            &event,
        )]
    }
}
