use hypha_core::{LocationId, MarketId, RepoError};
use hypha_inventory::{Location, LocationRepo, repo::keys};

use crate::table::{Table, no_unique_keys};

#[derive(Default)]
pub struct InMemoryLocationRepo {
    locations: Table<LocationId, Location>,
}

impl LocationRepo for InMemoryLocationRepo {
    async fn get(&self, id: LocationId) -> Result<Option<Location>, RepoError> {
        Ok(self.locations.get(&id))
    }

    async fn serving(&self, market: MarketId) -> Result<Vec<Location>, RepoError> {
        let mut serving: Vec<Location> = self
            .locations
            .all()
            .into_iter()
            .filter(|l| l.serves(market))
            .collect();
        serving.sort_by_key(|l| (l.priority(), l.id().as_uuid()));
        Ok(serving)
    }

    async fn insert(&self, location: &mut Location) -> Result<(), RepoError> {
        self.locations
            .insert(location.id(), location, keys::LOCATION_ID, &no_unique_keys)
    }

    async fn update(&self, location: &mut Location) -> Result<(), RepoError> {
        self.locations
            .update(location.id(), location, &no_unique_keys)
    }
}
