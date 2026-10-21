//! Personal data workflows over customers (design note 0032, §7): deleting a customer
//! without orders, erasing one with them (a request with a cancel window, run by a job),
//! and answering a data request with an export. A workflow crate: it composes customers,
//! orders, carts, payments, email records and custom fields, and writes each action's
//! aggregates in one batch.

pub mod commands;
pub mod erasure;
pub mod settings;

pub use commands::{CommandError, PrivacyPorts, delete_customer};
pub use erasure::{
    CustomerExport, Erased, ErasurePorts, Erasures, cancel_erasure, erase_customer, erase_due,
    export_customer, request_erasure,
};
pub use settings::PrivacySettings;
