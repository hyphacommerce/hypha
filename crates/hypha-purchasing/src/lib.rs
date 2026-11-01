//! Purchasing: the suppliers a store buys from, what each sells it, and the purchase
//! orders placed with them. A leaf crate on
//! core: it names variants and locations by their IDs, and takes the store's currency and
//! the variants' titles as plain values.

pub mod catalogue;
pub mod commands;
pub mod costs;
pub mod error;
pub mod events;
pub mod incoming;
pub mod purchase_order;
pub mod receipt;
pub mod repo;
pub mod supplier;

pub use catalogue::{SupplierVariant, SupplierVariantDraft};
pub use commands::{
    CommandError, LineInput, NewPurchaseOrder, SendPurchaseOrder, UpdatePurchaseOrder,
    UpdateSupplier, VariantInfo, apply_update, cancel_purchase_order, close_purchase_order,
    create_purchase_order, create_supplier, delete_purchase_order, delete_supplier,
    get_purchase_order, get_receipt, get_supplier, list_purchase_orders, list_receipts,
    list_supplier_variants, list_suppliers, order_purchase_order, remove_supplier_variant,
    send_purchase_order, set_supplier_variant, supplier_variant, update_purchase_order,
    update_supplier, variant_suppliers,
};
pub use costs::{CostAllocation, CostShare, LineBasis, extra_per_unit, landed_unit_cost};
pub use error::{Outstanding, PurchasingError};
pub use events::{
    PURCHASE_ORDER, PurchaseOrderCancelled, PurchaseOrderClosed, PurchaseOrderCreated,
    PurchaseOrderOrdered, PurchaseOrderReceiptStatusChanged, PurchaseOrderSent,
    PurchaseOrderUpdated, RECEIPT, ReceiptCancelled, ReceiptCreated, ReceiptLineEvent, SUPPLIER,
    ShipTo, ShipToAddress, SupplierCreated, SupplierDeleted, SupplierUpdated,
};
pub use incoming::{IncomingStock, incoming};
pub use purchase_order::{
    CostDraft, CostKind, ExtraCost, LineDraft, LineSnapshot, MAX_COSTS, MAX_LINES,
    MAX_MESSAGE_CHARS, MAX_RECIPIENTS, PurchaseOrder, PurchaseOrderDraft, PurchaseOrderLine,
    PurchaseOrderNumber, PurchaseOrderSnapshot, PurchaseOrderStatus, PurchaseOrderSummary,
    PurchaseOrderTotals, ReceiptBasis, ReceiptStatus, ReceivedQuantities, Revaluation, Sending,
};
pub use receipt::{NewReceipt, Receipt, ReceiptLine, ReceiptSnapshot, ReceiptState};
pub use repo::{
    HasPurchaseOrders, HasReceipts, HasSupplierVariants, HasSuppliers, PurchaseOrderFilter,
    PurchaseOrderRepo, PurchaseOrderSort, ReceiptRepo, StagePurchaseOrders, StageReceipts,
    SupplierFilter, SupplierRepo, SupplierVariantRepo, keys,
};
pub use supplier::{
    Contact, ContactDraft, MAX_CONTACTS, MAX_LEAD_TIME_DAYS, MAX_NET_DAYS, MAX_NOTES_CHARS,
    PaymentDue, PaymentTerms, PaymentTermsDraft, Supplier, SupplierDraft, SupplierSnapshot,
};
