//! What happens to suppliers and purchase orders, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{Event, PurchaseOrderId, SupplierId};
use serde::Serialize;

pub const SUPPLIER: &str = "supplier";
pub const PURCHASE_ORDER: &str = "purchase_order";

/// `supplier.created`.
#[derive(Debug, Serialize)]
pub struct SupplierCreated {
    pub supplier: SupplierId,
}

impl Event for SupplierCreated {
    const TYPE: &'static str = "supplier.created";
    const VERSION: u16 = 1;
}

/// `supplier.updated`: `fields` names what changed (`name`, `contacts`, `address`,
/// `currency`, `payment_terms`, `lead_time_days`, `tax_number`, `notes`, `archived`).
#[derive(Debug, Serialize)]
pub struct SupplierUpdated {
    pub supplier: SupplierId,
    pub fields: Vec<&'static str>,
}

impl Event for SupplierUpdated {
    const TYPE: &'static str = "supplier.updated";
    const VERSION: u16 = 1;
}

/// `supplier.deleted`: its catalogue went with it.
#[derive(Debug, Serialize)]
pub struct SupplierDeleted {
    pub supplier: SupplierId,
}

impl Event for SupplierDeleted {
    const TYPE: &'static str = "supplier.deleted";
    const VERSION: u16 = 1;
}

/// `purchase_order.created`: a draft.
#[derive(Debug, Serialize)]
pub struct PurchaseOrderCreated {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
}

impl Event for PurchaseOrderCreated {
    const TYPE: &'static str = "purchase_order.created";
    const VERSION: u16 = 1;
}

/// `purchase_order.updated`: `fields` names what changed (`destination`, `currency`,
/// `exchange_rate`, `payment_terms`, `expected_at`, `supplier_reference`,
/// `note_to_supplier`, `note`, `lines`, `costs`, `tax`, `status` when a closed order
/// reopens); `revision` counts the edits since it was ordered.
#[derive(Debug, Serialize)]
pub struct PurchaseOrderUpdated {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
    pub fields: Vec<&'static str>,
    pub revision: u32,
}

impl Event for PurchaseOrderUpdated {
    const TYPE: &'static str = "purchase_order.updated";
    const VERSION: u16 = 1;
}

/// `purchase_order.ordered`: placed with the supplier.
#[derive(Debug, Serialize)]
pub struct PurchaseOrderOrdered {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
}

impl Event for PurchaseOrderOrdered {
    const TYPE: &'static str = "purchase_order.ordered";
    const VERSION: u16 = 1;
}

/// `purchase_order.closed`: by hand (what was outstanding is cancelled) or once its
/// last line was received.
#[derive(Debug, Serialize)]
pub struct PurchaseOrderClosed {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
}

impl Event for PurchaseOrderClosed {
    const TYPE: &'static str = "purchase_order.closed";
    const VERSION: u16 = 1;
}

/// `purchase_order.cancelled`: nothing was received.
#[derive(Debug, Serialize)]
pub struct PurchaseOrderCancelled {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
}

impl Event for PurchaseOrderCancelled {
    const TYPE: &'static str = "purchase_order.cancelled";
    const VERSION: u16 = 1;
}

/// `purchase_order.receipt_status_changed`: `none`, `partial` or `received`.
#[derive(Debug, Serialize)]
pub struct PurchaseOrderReceiptStatusChanged {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
    pub from: &'static str,
    pub to: &'static str,
}

impl Event for PurchaseOrderReceiptStatusChanged {
    const TYPE: &'static str = "purchase_order.receipt_status_changed";
    const VERSION: u16 = 1;
}
