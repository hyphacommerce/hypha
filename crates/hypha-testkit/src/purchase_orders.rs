use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use hypha_core::{
    Keyset, Page, PageRequest, PurchaseOrderId, ReceiptId, Recorded, RepoError, SupplierId,
    VariantId, Versioned, stamp_insert, stamp_update,
};
use hypha_purchasing::{
    PurchaseOrder, PurchaseOrderFilter, PurchaseOrderLine, PurchaseOrderNumber, PurchaseOrderRepo,
    PurchaseOrderSort, PurchaseOrderStatus, PurchaseOrderSummary, Receipt, ReceiptRepo, keys,
};

use crate::{InMemoryOutbox, InMemorySuppliers, table::Table};

/// Purchase orders. They share the suppliers' store, so a supplier an order names can't
/// be deleted (its `in_use`) and an order's supplier must exist, as a database's foreign
/// key would have it.
#[derive(Clone)]
pub struct InMemoryPurchaseOrders {
    pub(crate) orders: Arc<Table<PurchaseOrderId, PurchaseOrder>>,
    last_number: Arc<AtomicI64>,
    pub(crate) suppliers: InMemorySuppliers,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryPurchaseOrders {
    /// Orders to `suppliers`' suppliers, writing their events to `outbox`.
    pub fn sharing(suppliers: &InMemorySuppliers, outbox: &InMemoryOutbox) -> Self {
        Self {
            orders: Arc::default(),
            // The first number is 1001, as the database's sequence starts.
            last_number: Arc::new(AtomicI64::new(1000)),
            suppliers: suppliers.clone(),
            outbox: outbox.clone(),
        }
    }

    /// Marks again which suppliers are named by an order.
    pub(crate) fn refresh_in_use(&self) {
        let named: Vec<SupplierId> = self
            .orders
            .all()
            .iter()
            .map(PurchaseOrder::supplier)
            .collect();
        let mut in_use = self
            .suppliers
            .in_use
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        in_use.clear();
        in_use.extend(named);
    }
}

/// One order per number.
fn unique_number(a: &PurchaseOrder, b: &PurchaseOrder) -> Option<&'static str> {
    (a.id() != b.id() && a.number() == b.number()).then_some(keys::PURCHASE_ORDER_NUMBER)
}

impl PurchaseOrderRepo for InMemoryPurchaseOrders {
    async fn get(&self, id: PurchaseOrderId) -> Result<Option<PurchaseOrder>, RepoError> {
        Ok(self.orders.get(&id))
    }

    async fn list(
        &self,
        filter: &PurchaseOrderFilter,
        sort: PurchaseOrderSort,
        page: PageRequest<Keyset<PurchaseOrderId>>,
    ) -> Result<Page<PurchaseOrderSummary>, RepoError> {
        if let Some(after) = &page.after
            && !sort.fits(&after.key)
        {
            return Err(RepoError::Backend(format!(
                "a {:?} position for a {sort:?} list",
                after.key
            )));
        }
        Ok(self.orders.page_sorted(
            |po| filter.matches(po),
            &page,
            sort.direction(),
            PurchaseOrder::summary,
            |s: &PurchaseOrderSummary| sort.key(s),
        ))
    }

    async fn next_number(&self) -> Result<PurchaseOrderNumber, RepoError> {
        let n = self.last_number.fetch_add(1, Ordering::SeqCst) + 1;
        PurchaseOrderNumber::new(n).map_err(|e| RepoError::Backend(e.to_string()))
    }

    async fn insert(&self, po: &mut PurchaseOrder) -> Result<(), RepoError> {
        if self.suppliers.suppliers.get(&po.supplier()).is_none() {
            return Err(RepoError::Reference(keys::PURCHASE_ORDER_SUPPLIER));
        }
        stamp_insert(po, self.outbox.now());
        let events = po.take_events();
        self.orders
            .insert(po.id(), po, keys::PURCHASE_ORDER_ID, &unique_number)?;
        self.refresh_in_use();
        self.outbox.write(events, Some(po.version()));
        Ok(())
    }

    async fn update(&self, po: &mut PurchaseOrder) -> Result<(), RepoError> {
        stamp_update(po, self.outbox.now());
        let events = po.take_events();
        self.orders.update(po.id(), po, &unique_number)?;
        self.outbox.write(events, Some(po.version()));
        Ok(())
    }

    async fn delete(&self, po: &mut PurchaseOrder) -> Result<(), RepoError> {
        {
            let mut rows = self.orders.lock();
            match rows.get(&po.id()) {
                Some(stored) if stored.version() == po.version() => {}
                _ => return Err(RepoError::StaleVersion),
            }
            rows.remove(&po.id());
        }
        self.refresh_in_use();
        self.outbox.write(po.take_events(), None);
        Ok(())
    }

    async fn last_line(
        &self,
        supplier: SupplierId,
        variant: VariantId,
    ) -> Result<Option<PurchaseOrderLine>, RepoError> {
        Ok(self
            .orders
            .all()
            .into_iter()
            .filter(|po| po.supplier() == supplier && po.status() != PurchaseOrderStatus::Cancelled)
            .filter_map(|po| po.line(variant).cloned().map(|line| (po.number(), line)))
            .max_by_key(|(number, _)| *number)
            .map(|(_, line)| line))
    }
}

/// Receipts. Written only through a batch, with their order, like the database's.
#[derive(Clone, Default)]
pub struct InMemoryReceipts {
    pub(crate) receipts: Arc<Table<ReceiptId, Receipt>>,
}

impl ReceiptRepo for InMemoryReceipts {
    async fn get(&self, id: ReceiptId) -> Result<Option<Receipt>, RepoError> {
        Ok(self.receipts.get(&id))
    }

    async fn for_purchase_order(&self, po: PurchaseOrderId) -> Result<Vec<Receipt>, RepoError> {
        let mut found: Vec<Receipt> = self
            .receipts
            .all()
            .into_iter()
            .filter(|r| r.purchase_order() == po)
            .collect();
        found.sort_by_key(Receipt::id);
        Ok(found)
    }
}
