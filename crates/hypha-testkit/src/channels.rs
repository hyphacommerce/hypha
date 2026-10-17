use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_channels::{Channel, ChannelRepo, keys};
use hypha_core::{
    ChannelId, ProductId, Recorded, RepoError, Versioned, stamp_insert, stamp_update,
};

use crate::{
    InMemoryOutbox,
    table::{Table, missing},
};

/// Publications: each channel's products.
pub(crate) type Publications = HashMap<ChannelId, HashSet<ProductId>>;

#[derive(Default)]
pub struct InMemoryChannelRepo {
    channels: Table<ChannelId, Channel>,
    /// Stored apart from the channels, like a separate table would be; shared with the
    /// product repo, whose list filters by channel (design note 0024, AU5).
    pub(crate) products: Arc<Mutex<Publications>>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemoryChannelRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// Publications, locked, for a batch (`InMemoryPorts::commit`).
    pub(crate) fn lock_products(&self) -> MutexGuard<'_, Publications> {
        self.products.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn exists(&self, id: ChannelId) -> bool {
        self.channels.get(&id).is_some()
    }
}

/// Names are unique by `Channel::name_key`.
fn unique(new: &Channel, other: &Channel) -> Option<&'static str> {
    (new.name_key() == other.name_key()).then_some(keys::CHANNEL_NAME)
}

impl ChannelRepo for InMemoryChannelRepo {
    async fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepoError> {
        Ok(self.channels.get(&id))
    }

    async fn find_by_name(&self, name_key: &str) -> Result<Option<Channel>, RepoError> {
        Ok(self.channels.find(|c| c.name_key() == name_key))
    }

    async fn list(&self) -> Result<Vec<Channel>, RepoError> {
        let mut all = self.channels.all();
        all.sort_by_key(Channel::id);
        Ok(all)
    }

    async fn insert(&self, channel: &mut Channel) -> Result<(), RepoError> {
        stamp_insert(channel, self.outbox.now());
        let events = channel.take_events();
        self.channels
            .insert(channel.id(), channel, keys::CHANNEL_ID, &unique)?;
        self.outbox.write(events, Some(channel.version()));
        Ok(())
    }

    async fn update(&self, channel: &mut Channel) -> Result<(), RepoError> {
        stamp_update(channel, self.outbox.now());
        let events = channel.take_events();
        self.channels.update(channel.id(), channel, &unique)?;
        self.outbox.write(events, Some(channel.version()));
        Ok(())
    }

    async fn publish(&self, channel: ChannelId, products: &[ProductId]) -> Result<(), RepoError> {
        if !self.exists(channel) {
            return Err(missing("channel", channel));
        }
        self.lock_products()
            .entry(channel)
            .or_default()
            .extend(products.iter().copied());
        Ok(())
    }

    async fn unpublish(&self, channel: ChannelId, products: &[ProductId]) -> Result<(), RepoError> {
        if let Some(published) = self.lock_products().get_mut(&channel) {
            for product in products {
                published.remove(product);
            }
        }
        Ok(())
    }

    async fn published(
        &self,
        channel: ChannelId,
        products: &[ProductId],
    ) -> Result<HashSet<ProductId>, RepoError> {
        let all = self.lock_products();
        let Some(published) = all.get(&channel) else {
            return Ok(HashSet::new());
        };
        Ok(products
            .iter()
            .copied()
            .filter(|p| published.contains(p))
            .collect())
    }

    async fn channels_of(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, Vec<ChannelId>>, RepoError> {
        let mut found: HashMap<ProductId, Vec<ChannelId>> = HashMap::new();
        for (channel, published) in self.lock_products().iter() {
            for product in products.iter().filter(|p| published.contains(p)) {
                found.entry(*product).or_default().push(*channel);
            }
        }
        for channels in found.values_mut() {
            channels.sort();
            channels.dedup();
        }
        Ok(found)
    }
}
