//! What happens to suppliers and purchase orders, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{
    Address, Email, Event, LocationId, PurchaseOrderId, ReceiptId, SupplierId, VariantId,
};
use serde::{Deserialize, Serialize};

pub const SUPPLIER: &str = "supplier";
pub const PURCHASE_ORDER: &str = "purchase_order";
pub const RECEIPT: &str = "receipt";

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

/// Where a sent order's goods go, as it was when sent: the destination's name and
/// address. A snapshot, so the email and its resends say what the supplier was told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipTo {
    /// The location's name.
    pub name: String,
    /// `null` for a location without an address.
    pub address: Option<ShipToAddress>,
}

/// A ship-to address's lines; `country` is the ISO 3166-1 alpha-2 code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipToAddress {
    /// The person to ask for: the address's first and last name.
    pub contact: Option<String>,
    pub company: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country: String,
    pub phone: Option<String>,
}

impl ShipTo {
    /// The location named `name`, at `address` when it has one.
    pub fn of(name: &str, address: Option<&Address>) -> Self {
        Self {
            name: name.to_string(),
            address: address.map(|a| {
                let parts = a.parts();
                let contact = match &parts.first_name {
                    Some(first) => format!("{first} {}", parts.last_name),
                    None => parts.last_name.clone(),
                };
                ShipToAddress {
                    contact: Some(contact).filter(|c| !c.trim().is_empty()),
                    company: parts.company.clone(),
                    line1: parts.line1.clone(),
                    line2: parts.line2.clone(),
                    city: parts.city.clone(),
                    province: parts.province.clone(),
                    postal_code: parts.postal_code.clone(),
                    country: a.country().as_str().to_string(),
                    phone: parts.phone.clone(),
                }
            }),
        }
    }
}

/// `purchase_order.sent`: emailed to the supplier. Each of `to` and `cc` gets the
/// order as it was at `revision`, with `message` and `ship_to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseOrderSent {
    pub purchase_order: PurchaseOrderId,
    pub supplier: SupplierId,
    pub revision: u32,
    pub to: Vec<Email>,
    pub cc: Vec<Email>,
    /// A note for this send, above the order.
    pub message: Option<String>,
    pub ship_to: Option<ShipTo>,
}

impl Event for PurchaseOrderSent {
    const TYPE: &'static str = "purchase_order.sent";
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

/// A receipt's line in its events.
#[derive(Debug, Clone, Serialize)]
pub struct ReceiptLineEvent {
    pub variant: VariantId,
    pub accepted: u32,
    pub rejected: u32,
}

/// `receipt.created`: goods arrived against a purchase order, at a location.
#[derive(Debug, Serialize)]
pub struct ReceiptCreated {
    pub receipt: ReceiptId,
    pub purchase_order: PurchaseOrderId,
    pub location: LocationId,
    pub lines: Vec<ReceiptLineEvent>,
}

impl Event for ReceiptCreated {
    const TYPE: &'static str = "receipt.created";
    const VERSION: u16 = 1;
}

/// `receipt.cancelled`: taken back, its units off stock and off the order's counts.
#[derive(Debug, Serialize)]
pub struct ReceiptCancelled {
    pub receipt: ReceiptId,
    pub purchase_order: PurchaseOrderId,
    pub location: LocationId,
    pub lines: Vec<ReceiptLineEvent>,
}

impl Event for ReceiptCancelled {
    const TYPE: &'static str = "receipt.cancelled";
    const VERSION: u16 = 1;
}
