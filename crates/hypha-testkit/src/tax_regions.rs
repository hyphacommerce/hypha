use hypha_core::{
    CountryCode, Recorded, RepoError, TaxRegionId, Versioned, stamp_insert, stamp_update,
};
use hypha_tax::{TaxRegion, TaxRegionRepo, repo::keys};

use crate::{InMemoryOutbox, table::Table};

#[derive(Default)]
pub struct InMemoryTaxRegionRepo {
    regions: Table<TaxRegionId, TaxRegion>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryTaxRegionRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

fn unique(new: &TaxRegion, other: &TaxRegion) -> Option<&'static str> {
    (new.country() == other.country()).then_some(keys::TAX_REGION_COUNTRY)
}

impl InMemoryTaxRegionRepo {
    /// The next `n` lookups by a unique key (and `list_all`, where there is one) find
    /// nothing, as if another request inserted the row right after: for testing a
    /// command that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.regions.hide_next_lookups(n);
    }
}

impl TaxRegionRepo for InMemoryTaxRegionRepo {
    async fn get(&self, id: TaxRegionId) -> Result<Option<TaxRegion>, RepoError> {
        Ok(self.regions.get(&id))
    }

    async fn get_by_country(&self, country: CountryCode) -> Result<Option<TaxRegion>, RepoError> {
        Ok(self.regions.find(|r| r.country() == country))
    }

    async fn list_all(&self) -> Result<Vec<TaxRegion>, RepoError> {
        Ok(self.regions.all_oldest_first())
    }

    async fn insert(&self, region: &mut TaxRegion) -> Result<(), RepoError> {
        stamp_insert(region, self.outbox.now());
        let events = region.take_events();
        self.regions
            .insert(region.id(), region, keys::TAX_REGION_ID, &unique)?;
        self.outbox.write(events, Some(region.version()));
        Ok(())
    }

    async fn update(&self, region: &mut TaxRegion) -> Result<(), RepoError> {
        stamp_update(region, self.outbox.now());
        let events = region.take_events();
        self.regions.update(region.id(), region, &unique)?;
        self.outbox.write(events, Some(region.version()));
        Ok(())
    }
}
