use hypha_core::{
    FulfillmentId, OrderId, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};
use hypha_fulfillment::{Fulfillment, FulfillmentRepo, repo::keys};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

#[derive(Default)]
pub struct InMemoryFulfillmentRepo {
    pub(crate) fulfillments: Table<FulfillmentId, Fulfillment>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryFulfillmentRepo {
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

impl FulfillmentRepo for InMemoryFulfillmentRepo {
    async fn get(&self, id: FulfillmentId) -> Result<Option<Fulfillment>, RepoError> {
        Ok(self.fulfillments.get(&id))
    }

    async fn list_for_order(&self, order: OrderId) -> Result<Vec<Fulfillment>, RepoError> {
        let mut found: Vec<Fulfillment> = self
            .fulfillments
            .all()
            .into_iter()
            .filter(|f| f.order() == order)
            .collect();
        found.sort_by_key(|f| std::cmp::Reverse(f.id().as_uuid()));
        Ok(found)
    }

    async fn insert(&self, fulfillment: &mut Fulfillment) -> Result<(), RepoError> {
        stamp_insert(fulfillment, self.outbox.now());
        let events = fulfillment.take_events();
        self.fulfillments.insert(
            fulfillment.id(),
            fulfillment,
            keys::FULFILLMENT_ID,
            &no_unique_keys,
        )?;
        self.outbox.write(events, Some(fulfillment.version()));
        Ok(())
    }

    async fn update(&self, fulfillment: &mut Fulfillment) -> Result<(), RepoError> {
        stamp_update(fulfillment, self.outbox.now());
        let events = fulfillment.take_events();
        self.fulfillments
            .update(fulfillment.id(), fulfillment, &no_unique_keys)?;
        self.outbox.write(events, Some(fulfillment.version()));
        Ok(())
    }
}
