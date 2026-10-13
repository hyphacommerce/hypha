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
    async fn insert(&self, region: &mut TaxRegion) -> Result<(), RepoError>;
    async fn update(&self, region: &mut TaxRegion) -> Result<(), RepoError>;
}
