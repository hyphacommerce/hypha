use hypha_core::{CountryCode, RepoError, TaxRegionId};
use hypha_tax::{TaxRegion, TaxRegionRepo, repo::keys};

use crate::table::Table;

#[derive(Default)]
pub struct InMemoryTaxRegionRepo {
    regions: Table<TaxRegionId, TaxRegion>,
}

fn unique(new: &TaxRegion, other: &TaxRegion) -> Option<&'static str> {
    (new.country() == other.country()).then_some(keys::TAX_REGION_COUNTRY)
}

impl TaxRegionRepo for InMemoryTaxRegionRepo {
    async fn get(&self, id: TaxRegionId) -> Result<Option<TaxRegion>, RepoError> {
        Ok(self.regions.get(&id))
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<TaxRegion>, RepoError> {
        Ok(self.regions.find(|r| r.country() == country))
    }

    async fn insert(&self, region: &mut TaxRegion) -> Result<(), RepoError> {
        self.regions
            .insert(region.id(), region, keys::TAX_REGION_ID, &unique)
    }

    async fn update(&self, region: &mut TaxRegion) -> Result<(), RepoError> {
        self.regions.update(region.id(), region, &unique)
    }
}
