use std::collections::{HashMap, HashSet};

use hypha_core::{ChannelId, ProductId, RepoError};

use crate::Channel;

/// Uniqueness rules a `ChannelRepo` enforces, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const CHANNEL_ID: &str = "channel_id";
    /// Names are unique ignoring case (`Channel::name_key`): "Online Store" and
    /// "online store" are one.
    pub const CHANNEL_NAME: &str = "channel_name";
}

/// Channels, plus the products each one sells (its publications).
///
/// Publications are read and written in batches (slices in, sets out), like catalog
/// membership: one query however many products a cart or a sync batch holds.
#[allow(async_fn_in_trait)]
pub trait ChannelRepo {
    async fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepoError>;
    /// The channel whose `Channel::name_key` is `name_key`, if any.
    async fn find_by_name(&self, name_key: &str) -> Result<Option<Channel>, RepoError>;
    /// Every channel, oldest first. A store has a handful.
    async fn list(&self) -> Result<Vec<Channel>, RepoError>;
    /// Stores a new channel (version 1). Enforces `keys`.
    async fn insert(&self, channel: &mut Channel) -> Result<(), RepoError>;
    /// Stores changes and bumps the version, or fails with `StaleVersion`. Enforces
    /// `CHANNEL_NAME`.
    async fn update(&self, channel: &mut Channel) -> Result<(), RepoError>;

    /// Publishes products to the channel; already published ones are ignored. Records
    /// no event: commands stage it (`StagePublications`) with
    /// `channel.products_published`; this is for setting up data.
    ///
    /// Precondition: the channel exists. Otherwise adapters fail rather than store
    /// orphaned publications.
    async fn publish(&self, channel: ChannelId, products: &[ProductId]) -> Result<(), RepoError>;
    /// Unpublishes products; ones not published are ignored. Records no event, like
    /// `publish`.
    async fn unpublish(&self, channel: ChannelId, products: &[ProductId]) -> Result<(), RepoError>;
    /// Which of `products` the channel sells.
    async fn published(
        &self,
        channel: ChannelId,
        products: &[ProductId],
    ) -> Result<HashSet<ProductId>, RepoError>;
    /// Each of `products`' channels, sorted; a product in none is absent.
    async fn channels_of(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, Vec<ChannelId>>, RepoError>;
}

/// Stages publications in a `hypha_core::HasBatch` batch, with their events
/// (`StageEvents`). Preconditions as `ChannelRepo::publish`: the channel exists.
pub trait StagePublications {
    fn publish_products(&mut self, channel: ChannelId, products: &[ProductId]);
    fn unpublish_products(&mut self, channel: ChannelId, products: &[ProductId]);
}

pub trait HasChannels {
    type Channels: ChannelRepo;
    fn channels(&self) -> &Self::Channels;
}

impl<T: HasChannels + ?Sized> HasChannels for &T {
    type Channels = T::Channels;
    fn channels(&self) -> &Self::Channels {
        (**self).channels()
    }
}
