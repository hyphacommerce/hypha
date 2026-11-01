use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_core::{
    Page, PageRequest, Recorded, RepoError, SupplierId, VariantId, Versioned, stamp_insert,
    stamp_update,
};
use hypha_purchasing::{
    Supplier, SupplierFilter, SupplierRepo, SupplierVariant, SupplierVariantRepo, keys,
};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys, page_of},
};

/// Suppliers and their catalogues, which a database keeps in a table of their own.
#[derive(Default, Clone)]
pub struct InMemorySuppliers {
    pub(crate) suppliers: Arc<Table<SupplierId, Supplier>>,
    pub(crate) rows: Arc<Mutex<BTreeMap<(SupplierId, VariantId), SupplierVariant>>>,
    /// Suppliers a purchase order names: deleting one is refused, as a database's
    /// foreign key would.
    pub(crate) in_use: Arc<Mutex<HashSet<SupplierId>>>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemorySuppliers {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    fn lock_rows(&self) -> MutexGuard<'_, BTreeMap<(SupplierId, VariantId), SupplierVariant>> {
        self.rows.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SupplierRepo for InMemorySuppliers {
    async fn get(&self, id: SupplierId) -> Result<Option<Supplier>, RepoError> {
        Ok(self.suppliers.get(&id))
    }

    async fn list(
        &self,
        filter: &SupplierFilter,
        page: PageRequest<SupplierId>,
    ) -> Result<Page<Supplier>, RepoError> {
        Ok(self
            .suppliers
            .page(|s| filter.matches(s), page, Clone::clone))
    }

    async fn insert(&self, supplier: &mut Supplier) -> Result<(), RepoError> {
        stamp_insert(supplier, self.outbox.now());
        let events = supplier.take_events();
        self.suppliers
            .insert(supplier.id(), supplier, keys::SUPPLIER_ID, &no_unique_keys)?;
        self.outbox.write(events, Some(supplier.version()));
        Ok(())
    }

    async fn update(&self, supplier: &mut Supplier) -> Result<(), RepoError> {
        stamp_update(supplier, self.outbox.now());
        let events = supplier.take_events();
        self.suppliers
            .update(supplier.id(), supplier, &no_unique_keys)?;
        self.outbox.write(events, Some(supplier.version()));
        Ok(())
    }

    async fn delete(&self, supplier: &mut Supplier) -> Result<(), RepoError> {
        let mut rows = self.suppliers.lock();
        match rows.get(&supplier.id()) {
            Some(stored) if stored.version() == supplier.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        if self
            .in_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&supplier.id())
        {
            return Err(RepoError::Reference(keys::SUPPLIER_IN_USE));
        }
        rows.remove(&supplier.id());
        self.lock_rows().retain(|(s, _), _| *s != supplier.id());
        self.outbox.write(supplier.take_events(), None);
        Ok(())
    }
}

impl SupplierVariantRepo for InMemorySuppliers {
    async fn put(&self, row: &SupplierVariant) -> Result<(), RepoError> {
        if self.suppliers.lock().get(&row.supplier()).is_none() {
            return Err(RepoError::Reference(keys::CATALOGUE_SUPPLIER));
        }
        self.lock_rows()
            .insert((row.supplier(), row.variant()), row.clone());
        Ok(())
    }

    async fn get(
        &self,
        supplier: SupplierId,
        variant: VariantId,
    ) -> Result<Option<SupplierVariant>, RepoError> {
        Ok(self.lock_rows().get(&(supplier, variant)).cloned())
    }

    async fn remove(&self, supplier: SupplierId, variant: VariantId) -> Result<bool, RepoError> {
        Ok(self.lock_rows().remove(&(supplier, variant)).is_some())
    }

    async fn list(
        &self,
        supplier: SupplierId,
        page: PageRequest<VariantId>,
    ) -> Result<Page<SupplierVariant>, RepoError> {
        let rows = self.lock_rows();
        let entries = rows
            .iter()
            .filter(|((s, _), _)| *s == supplier)
            .map(|((_, v), row)| (*v, row.clone()));
        let found = page_of(entries, page);
        Ok(Page {
            items: found.items.into_iter().map(|(_, row)| row).collect(),
            has_more: found.has_more,
        })
    }

    async fn for_variant(&self, variant: VariantId) -> Result<Vec<SupplierVariant>, RepoError> {
        let mut found: Vec<SupplierVariant> = self
            .lock_rows()
            .values()
            .filter(|row| row.variant() == variant)
            .cloned()
            .collect();
        found.sort_by_key(SupplierVariant::supplier);
        Ok(found)
    }

    async fn has_costs(&self, supplier: SupplierId) -> Result<bool, RepoError> {
        Ok(self
            .lock_rows()
            .values()
            .any(|row| row.supplier() == supplier && row.unit_cost().is_some()))
    }

    async fn remove_variants(&self, variants: &[VariantId]) -> Result<(), RepoError> {
        self.lock_rows().retain(|(_, v), _| !variants.contains(v));
        Ok(())
    }
}
