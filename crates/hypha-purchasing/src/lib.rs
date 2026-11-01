//! Purchasing: the suppliers a store buys from and what each sells it. A leaf crate on
//! core: it names variants and locations by their IDs, and takes the store's currency and
//! the variants' titles as plain values.

pub mod catalogue;
pub mod commands;
pub mod error;
pub mod events;
pub mod repo;
pub mod supplier;

pub use catalogue::{SupplierVariant, SupplierVariantDraft};
pub use commands::{
    CommandError, UpdateSupplier, create_supplier, delete_supplier, get_supplier,
    list_supplier_variants, list_suppliers, remove_supplier_variant, set_supplier_variant,
    supplier_variant, update_supplier, variant_suppliers,
};
pub use error::PurchasingError;
pub use events::{SUPPLIER, SupplierCreated, SupplierDeleted, SupplierUpdated};
pub use repo::{
    HasSupplierVariants, HasSuppliers, SupplierFilter, SupplierRepo, SupplierVariantRepo, keys,
};
pub use supplier::{
    Contact, ContactDraft, MAX_CONTACTS, MAX_LEAD_TIME_DAYS, MAX_NET_DAYS, MAX_NOTES_CHARS,
    PaymentDue, PaymentTerms, PaymentTermsDraft, Supplier, SupplierDraft, SupplierSnapshot,
};
