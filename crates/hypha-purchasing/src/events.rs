//! What happens to suppliers, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{Event, SupplierId};
use serde::Serialize;

pub const SUPPLIER: &str = "supplier";

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
