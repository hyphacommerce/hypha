use hypha_core::{
    Direction, Keyset, LocationId, Page, PageRequest, PurchaseOrderId, ReceiptId, RepoError,
    SortKey, SupplierId, VariantId,
};

use crate::{
    PurchaseOrder, PurchaseOrderLine, PurchaseOrderNumber, PurchaseOrderStatus,
    PurchaseOrderSummary, Receipt, ReceiptStatus, Supplier, SupplierVariant,
};

/// Rules a purchasing store enforces, reported as `RepoError::Duplicate(key)` or
/// `RepoError::Reference(key)`.
pub mod keys {
    pub const SUPPLIER_ID: &str = "supplier_id";
    /// One catalogue row per (supplier, variant).
    pub const SUPPLIER_VARIANT: &str = "supplier_variant";
    /// A catalogue row names a supplier that exists.
    pub const CATALOGUE_SUPPLIER: &str = "supplier_variant_supplier";
    /// A supplier named by a purchase order isn't deleted, so the order keeps its link.
    pub const SUPPLIER_IN_USE: &str = "supplier_in_use";
    pub const PURCHASE_ORDER_ID: &str = "purchase_order_id";
    /// One purchase order per number.
    pub const PURCHASE_ORDER_NUMBER: &str = "purchase_order_number";
    /// A purchase order names a supplier that exists.
    pub const PURCHASE_ORDER_SUPPLIER: &str = "purchase_order_supplier";
    pub const RECEIPT_ID: &str = "receipt_id";
    /// A receipt names a purchase order that exists.
    pub const RECEIPT_PURCHASE_ORDER: &str = "receipt_purchase_order";
}

/// Which suppliers a list wants: all that match every field set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupplierFilter {
    pub archived: Option<bool>,
    /// In the name, ignoring case.
    pub q: Option<String>,
}

impl SupplierFilter {
    /// The rule every adapter follows.
    pub fn matches(&self, supplier: &Supplier) -> bool {
        self.archived.is_none_or(|a| supplier.archived() == a)
            && self.q.as_deref().is_none_or(|q| {
                supplier
                    .name()
                    .to_lowercase()
                    .contains(&q.trim().to_lowercase())
            })
    }
}

#[allow(async_fn_in_trait)]
pub trait SupplierRepo {
    async fn get(&self, id: SupplierId) -> Result<Option<Supplier>, RepoError>;
    /// A page of the suppliers `filter` matches, newest first.
    async fn list(
        &self,
        filter: &SupplierFilter,
        page: PageRequest<SupplierId>,
    ) -> Result<Page<Supplier>, RepoError>;
    /// Stores a new supplier (version 1), writing its events.
    async fn insert(&self, supplier: &mut Supplier) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, supplier: &mut Supplier) -> Result<(), RepoError>;
    /// Removes it with its catalogue, writing its events, when still at the version
    /// loaded; otherwise `StaleVersion`. Refused with `Reference(SUPPLIER_IN_USE)` while a
    /// purchase order names it, in the same step as the delete.
    async fn delete(&self, supplier: &mut Supplier) -> Result<(), RepoError>;
}

/// Suppliers' catalogues. Rows record no events: they're terms, not happenings, and a
/// purchase order copies what it uses.
#[allow(async_fn_in_trait)]
pub trait SupplierVariantRepo {
    /// Stores the row, replacing the one for the same (supplier, variant).
    ///
    /// Precondition: the supplier exists. Otherwise it fails with
    /// `Reference(CATALOGUE_SUPPLIER)` rather than store an orphan.
    async fn put(&self, row: &SupplierVariant) -> Result<(), RepoError>;
    async fn get(
        &self,
        supplier: SupplierId,
        variant: VariantId,
    ) -> Result<Option<SupplierVariant>, RepoError>;
    /// Removes the row; whether there was one.
    async fn remove(&self, supplier: SupplierId, variant: VariantId) -> Result<bool, RepoError>;
    /// A page of the supplier's rows, newest variant first.
    async fn list(
        &self,
        supplier: SupplierId,
        page: PageRequest<VariantId>,
    ) -> Result<Page<SupplierVariant>, RepoError>;
    /// Every supplier's row for `variant`, by supplier ID.
    async fn for_variant(&self, variant: VariantId) -> Result<Vec<SupplierVariant>, RepoError>;
    /// Whether any of the supplier's rows has a cost.
    async fn has_costs(&self, supplier: SupplierId) -> Result<bool, RepoError>;
    /// Removes every supplier's rows for `variants` (they were removed from their
    /// products); none is fine.
    async fn remove_variants(&self, variants: &[VariantId]) -> Result<(), RepoError>;
}

/// Which purchase orders a list wants: all that match every field set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurchaseOrderFilter {
    pub supplier: Option<SupplierId>,
    pub status: Option<PurchaseOrderStatus>,
    pub receipt_status: Option<ReceiptStatus>,
    pub destination: Option<LocationId>,
    /// Orders with a line for it.
    pub variant: Option<VariantId>,
}

impl PurchaseOrderFilter {
    /// The rule every adapter follows.
    pub fn matches(&self, po: &PurchaseOrder) -> bool {
        self.supplier.is_none_or(|s| po.supplier() == s)
            && self.status.is_none_or(|s| po.status() == s)
            && self.receipt_status.is_none_or(|s| po.receipt_status() == s)
            && self.destination.is_none_or(|d| po.destination() == Some(d))
            && self.variant.is_none_or(|v| po.line(v).is_some())
    }
}

/// How a list of purchase orders is sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PurchaseOrderSort {
    /// Highest number first: the newest.
    #[default]
    NumberDesc,
    NumberAsc,
    /// Soonest expected first; orders with no expected date last.
    ExpectedAt,
}

impl PurchaseOrderSort {
    /// What an order without an expected date sorts as: after every date.
    pub const NOT_EXPECTED: i64 = i64::MAX;

    pub fn direction(self) -> Direction {
        match self {
            PurchaseOrderSort::NumberDesc => Direction::Descending,
            PurchaseOrderSort::NumberAsc | PurchaseOrderSort::ExpectedAt => Direction::Ascending,
        }
    }

    /// The key `summary` sorts by: its number, or its expected time in microseconds.
    pub fn key(self, summary: &PurchaseOrderSummary) -> SortKey {
        match self {
            PurchaseOrderSort::NumberDesc | PurchaseOrderSort::NumberAsc => {
                SortKey::Int(summary.number.value())
            }
            PurchaseOrderSort::ExpectedAt => SortKey::Int(
                summary
                    .expected_at
                    .map_or(Self::NOT_EXPECTED, |t| t.unix_micros()),
            ),
        }
    }

    /// Whether `key` is the kind this sort uses: every sort uses whole numbers.
    pub fn fits(self, key: &SortKey) -> bool {
        matches!(key, SortKey::Int(_))
    }
}

#[allow(async_fn_in_trait)]
pub trait PurchaseOrderRepo {
    async fn get(&self, id: PurchaseOrderId) -> Result<Option<PurchaseOrder>, RepoError>;
    /// A page of the summaries (`PurchaseOrder::summary`) of the orders `filter` matches,
    /// sorted `sort` from after `page.after` (its key by `PurchaseOrderSort::key`, then
    /// the ID, in the sort's direction). A position whose key doesn't fit is a `Backend`
    /// error.
    async fn list(
        &self,
        filter: &PurchaseOrderFilter,
        sort: PurchaseOrderSort,
        page: PageRequest<Keyset<PurchaseOrderId>>,
    ) -> Result<Page<PurchaseOrderSummary>, RepoError>;
    /// The next number: unique and increasing, never handed out twice, even if the order
    /// it was for is never stored.
    async fn next_number(&self) -> Result<PurchaseOrderNumber, RepoError>;
    /// Stores a new order (version 1) with its events. Its supplier must exist
    /// (`Reference(PURCHASE_ORDER_SUPPLIER)`); a number already used is
    /// `Duplicate(PURCHASE_ORDER_NUMBER)`.
    async fn insert(&self, po: &mut PurchaseOrder) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`.
    async fn update(&self, po: &mut PurchaseOrder) -> Result<(), RepoError>;
    /// Removes it (a draft) when still at the version loaded; otherwise `StaleVersion`.
    async fn delete(&self, po: &mut PurchaseOrder) -> Result<(), RepoError>;
    /// `variant`'s line on the supplier's newest order that has one (by number), any
    /// status but cancelled: what a new line for it starts from.
    async fn last_line(
        &self,
        supplier: SupplierId,
        variant: VariantId,
    ) -> Result<Option<PurchaseOrderLine>, RepoError>;
}

/// Stages purchase order writes in a `hypha_core::HasBatch` batch, with their events.
pub trait StagePurchaseOrders {
    /// Stages what `PurchaseOrderRepo::update` would write, and moves `po` to the version
    /// it has once the batch commits.
    fn update_purchase_order(&mut self, po: &mut PurchaseOrder);
}

/// Receipts: what arrived against purchase orders. Written only in a batch with their
/// order and the stock they changed (`StageReceipts`).
#[allow(async_fn_in_trait)]
pub trait ReceiptRepo {
    async fn get(&self, id: ReceiptId) -> Result<Option<Receipt>, RepoError>;
    /// The order's receipts, oldest first (by ID). An order has a handful.
    async fn for_purchase_order(&self, po: PurchaseOrderId) -> Result<Vec<Receipt>, RepoError>;
}

/// Stages receipt writes in a `hypha_core::HasBatch` batch, with their events.
pub trait StageReceipts {
    /// Stages a new receipt (version 1). Its order must exist
    /// (`Reference(RECEIPT_PURCHASE_ORDER)`), its ID be new (`Duplicate(RECEIPT_ID)`).
    fn insert_receipt(&mut self, receipt: &mut Receipt);
    /// Stages what changed (it was taken back), checking the version it was loaded at.
    fn update_receipt(&mut self, receipt: &mut Receipt);
}

pub trait HasReceipts {
    type Receipts: ReceiptRepo;
    fn receipts(&self) -> &Self::Receipts;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasReceipts + ?Sized> HasReceipts for &T {
    type Receipts = T::Receipts;
    fn receipts(&self) -> &Self::Receipts {
        (**self).receipts()
    }
}

pub trait HasPurchaseOrders {
    type PurchaseOrders: PurchaseOrderRepo;
    fn purchase_orders(&self) -> &Self::PurchaseOrders;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasPurchaseOrders + ?Sized> HasPurchaseOrders for &T {
    type PurchaseOrders = T::PurchaseOrders;
    fn purchase_orders(&self) -> &Self::PurchaseOrders {
        (**self).purchase_orders()
    }
}

pub trait HasSuppliers {
    type Suppliers: SupplierRepo;
    fn suppliers(&self) -> &Self::Suppliers;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasSuppliers + ?Sized> HasSuppliers for &T {
    type Suppliers = T::Suppliers;
    fn suppliers(&self) -> &Self::Suppliers {
        (**self).suppliers()
    }
}

pub trait HasSupplierVariants {
    type SupplierVariants: SupplierVariantRepo;
    fn supplier_variants(&self) -> &Self::SupplierVariants;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasSupplierVariants + ?Sized> HasSupplierVariants for &T {
    type SupplierVariants = T::SupplierVariants;
    fn supplier_variants(&self) -> &Self::SupplierVariants {
        (**self).supplier_variants()
    }
}
