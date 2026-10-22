use hypha_core::{
    Direction, InventoryItemId, Keyset, Page, PageRequest, Recorded, RepoError, VariantId,
    Versioned, stamp_insert, stamp_update,
};
use hypha_inventory::{
    InventoryFilter, InventoryItem, InventoryItemRepo, InventoryItemSummary, repo::keys,
};

use crate::{InMemoryOutbox, table::Table};

#[derive(Default)]
pub struct InMemoryInventoryItemRepo {
    pub(crate) items: Table<InventoryItemId, InventoryItem>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryInventoryItemRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

/// One item per variant.
pub(crate) fn unique(new: &InventoryItem, other: &InventoryItem) -> Option<&'static str> {
    (new.variant() == other.variant()).then_some(keys::INVENTORY_ITEM_VARIANT)
}

impl InMemoryInventoryItemRepo {
    /// The next `n` lookups by a unique key (and `list_all`, where there is one) find
    /// nothing, as if another request inserted the row right after: for testing a
    /// command that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.items.hide_next_lookups(n);
    }
}

impl InventoryItemRepo for InMemoryInventoryItemRepo {
    async fn get(&self, id: InventoryItemId) -> Result<Option<InventoryItem>, RepoError> {
        Ok(self.items.get(&id))
    }

    async fn get_by_variant(&self, variant: VariantId) -> Result<Option<InventoryItem>, RepoError> {
        Ok(self.items.find(|i| i.variant() == variant))
    }

    async fn for_variants(&self, variants: &[VariantId]) -> Result<Vec<InventoryItem>, RepoError> {
        Ok(self
            .items
            .all()
            .into_iter()
            .filter(|i| variants.contains(&i.variant()))
            .collect())
    }

    async fn list(
        &self,
        filter: &InventoryFilter,
        page: PageRequest<Keyset<InventoryItemId>>,
    ) -> Result<Page<InventoryItemSummary>, RepoError> {
        if let Some(after) = &page.after
            && !InventoryFilter::fits(&after.key)
        {
            return Err(RepoError::Backend(format!(
                "a {:?} position for an inventory list",
                after.key
            )));
        }
        Ok(self.items.page_sorted(
            |i| filter.matches(i),
            &page,
            Direction::Ascending,
            InventoryItem::summary,
            |s: &InventoryItemSummary| filter.key(s),
        ))
    }

    async fn insert(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        stamp_insert(item, self.outbox.now());
        let events = item.take_events();
        self.items
            .insert(item.id(), item, keys::INVENTORY_ITEM_ID, &unique)?;
        self.outbox.write(events, Some(item.version()));
        Ok(())
    }

    async fn delete(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        let events = item.take_events();
        self.items.remove(&item.id());
        self.outbox.write(events, None);
        Ok(())
    }

    async fn update(&self, item: &mut InventoryItem) -> Result<(), RepoError> {
        stamp_update(item, self.outbox.now());
        let events = item.take_events();
        self.items.update(item.id(), item, &unique)?;
        self.outbox.write(events, Some(item.version()));
        Ok(())
    }

    async fn update_many(&self, items: &mut [InventoryItem]) -> Result<(), RepoError> {
        let now = self.outbox.now();
        for item in items.iter_mut() {
            stamp_update(item, now);
        }
        let events: Vec<_> = items.iter_mut().map(|i| i.take_events()).collect();
        self.items.update_many(items, InventoryItem::id, &unique)?;
        for (item, events) in items.iter().zip(events) {
            self.outbox.write(events, Some(item.version()));
        }
        Ok(())
    }
}
