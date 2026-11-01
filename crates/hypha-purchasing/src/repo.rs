use hypha_core::{Page, PageRequest, RepoError, SupplierId, VariantId};

use crate::{Supplier, SupplierVariant};

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
