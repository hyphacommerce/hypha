use std::collections::HashMap;

use hypha_core::{CountryCode, ProductId, RepoError, TaxClassId, TaxRegionId};

use crate::{TaxClass, TaxRegion};

pub mod keys {
    pub const TAX_CLASS_ID: &str = "tax_class_id";
    pub const TAX_REGION_ID: &str = "tax_region_id";
    pub const TAX_REGION_COUNTRY: &str = "tax_region_country";
}

#[allow(async_fn_in_trait)]
pub trait TaxClassRepo {
    async fn get(&self, id: TaxClassId) -> Result<Option<TaxClass>, RepoError>;
    async fn insert(&self, class: &mut TaxClass) -> Result<(), RepoError>;
    /// Every class, oldest first (by ID). A store has a handful.
    async fn list_all(&self) -> Result<Vec<TaxClass>, RepoError>;
    /// Sets the products' class (`None`: the default rate). Records no event: commands
    /// stage it (`StageTaxAssignments`) with `product.tax_class_set`; this is for
    /// setting up data.
    async fn assign(
        &self,
        products: &[ProductId],
        class: Option<TaxClassId>,
    ) -> Result<(), RepoError>;
    async fn classes_for(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, TaxClassId>, RepoError>;
}

#[allow(async_fn_in_trait)]
pub trait TaxRegionRepo {
    async fn get(&self, id: TaxRegionId) -> Result<Option<TaxRegion>, RepoError>;
    async fn get_by_country(&self, country: CountryCode) -> Result<Option<TaxRegion>, RepoError>;
    /// Every region with its class rates, oldest first (by ID); one per country at most.
    async fn list_all(&self) -> Result<Vec<TaxRegion>, RepoError>;
    async fn insert(&self, region: &mut TaxRegion) -> Result<(), RepoError>;
    async fn update(&self, region: &mut TaxRegion) -> Result<(), RepoError>;
}

/// Whatever holds the `TaxClassRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use; `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasTaxClasses {
    type TaxClasses: TaxClassRepo;
    fn tax_classes(&self) -> &Self::TaxClasses;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasTaxClasses + ?Sized> HasTaxClasses for &T {
    type TaxClasses = T::TaxClasses;
    fn tax_classes(&self) -> &Self::TaxClasses {
        (**self).tax_classes()
    }
}

/// Whatever holds the `TaxRegionRepo`: a host's per-request ports, a test world. Commands
/// bind just the accessors they use; `hypha_cart::CartPorts` is
/// the union of all of them.
pub trait HasTaxRegions {
    type TaxRegions: TaxRegionRepo;
    fn tax_regions(&self) -> &Self::TaxRegions;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasTaxRegions + ?Sized> HasTaxRegions for &T {
    type TaxRegions = T::TaxRegions;
    fn tax_regions(&self) -> &Self::TaxRegions {
        (**self).tax_regions()
    }
}

/// Tax-class assignments as a batch writes them, so the change and its events commit
/// together.
pub trait StageTaxAssignments {
    fn assign_tax_class(&mut self, products: &[ProductId], class: Option<TaxClassId>);
}
