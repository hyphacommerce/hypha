//! Search's ports (design note 0019, sections 4 to 6): the engine, the queue of what
//! to rebuild, and the merchant's settings.

use hypha_core::{CategoryId, ChannelId, MarketId, ProductId, RepoError, Timestamp, VariantId};

use crate::{IndexError, SearchDocument, SearchQuery, SearchResults, SearchSettings};

/// A search engine: Postgres built in, Meilisearch and others as adapters. Every engine
/// runs the `search_index` contract, which holds it to `reference::search`.
///
/// Not part of a request's ports (`HasXxx`): a host picks one engine for its life and
/// passes it to the commands and the sync job, like the `WebhookSender`. An engine in
/// the database can be the request's ports themselves.
#[allow(async_fn_in_trait)]
pub trait SearchIndex {
    /// Makes `documents` the documents of `products` in `market`: each product's
    /// documents there are replaced by its ones in `documents` (removed when it has
    /// none). Every document is of one of `products`, in `market`. One call per market
    /// for a whole batch, so an engine can write it at once.
    async fn replace(
        &self,
        market: MarketId,
        products: &[ProductId],
        documents: &[SearchDocument],
    ) -> Result<(), IndexError>;

    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError>;

    /// The option names (lowercased) documents in `market` have, most documents
    /// first, then by name: the default facets. With a `channel`, only its documents
    /// count (design note 0020), so a storefront's facets are its own products'.
    /// `reference::option_names` says exactly what.
    async fn option_names(
        &self,
        market: MarketId,
        channel: Option<ChannelId>,
    ) -> Result<Vec<String>, IndexError>;

    /// How many documents it holds, over every market: none means a new engine, which
    /// the host fills with a full rebuild.
    async fn count(&self) -> Result<u64, IndexError>;
}

/// A borrowed engine is an engine, so a host can lend the request's ports out as one.
impl<T: SearchIndex + ?Sized> SearchIndex for &T {
    async fn replace(
        &self,
        market: MarketId,
        products: &[ProductId],
        documents: &[SearchDocument],
    ) -> Result<(), IndexError> {
        (**self).replace(market, products, documents).await
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError> {
        (**self).search(query).await
    }

    async fn option_names(
        &self,
        market: MarketId,
        channel: Option<ChannelId>,
    ) -> Result<Vec<String>, IndexError> {
        (**self).option_names(market, channel).await
    }

    async fn count(&self) -> Result<u64, IndexError> {
        (**self).count().await
    }
}

/// What a mark asks to rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// One product's documents in every market.
    Product(ProductId),
    /// The product of this variant (resolved when the mark is worked).
    Variant(VariantId),
    /// The products in this category or below it, whose ancestor lists changed when it
    /// moved (design note 0022, section 3; resolved when the mark is worked).
    Category(CategoryId),
    /// Every product.
    All,
}

/// A queued target, as `SearchQueue::lease` hands it out. `generation` goes up when
/// the target is marked again, so finishing an older lease keeps the newer mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub target: Target,
    pub generation: i64,
}

/// What needs rebuilding (design note 0019, section 5): one row per target however
/// often it's marked, which is how 500 checkouts of one shirt become one rebuild.
#[allow(async_fn_in_trait)]
pub trait SearchQueue {
    /// Queues `targets`: new ones are added, queued ones get a new generation (so a
    /// lease working on the old one leaves them queued).
    async fn mark(&self, targets: &[Target]) -> Result<(), RepoError>;
    /// Up to `limit` marks whose lease is free (none, or ended by `now`), oldest marked
    /// first, leased until `until`.
    async fn lease(
        &self,
        now: Timestamp,
        until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Mark>, RepoError>;
    /// Removes the marks still at the generation leased; a mark marked again since
    /// stays, with its lease freed so the next run takes it. Returns the targets whose
    /// mark was gone: another runner took and finished it after this lease ran out, so
    /// what this runner wrote may be older than that runner's work (mark them again).
    async fn finish(&self, marks: &[Mark]) -> Result<Vec<Target>, RepoError>;
    /// How many targets are queued.
    async fn queued(&self) -> Result<u64, RepoError>;
}

/// The store's search settings: none stored until the merchant first saves.
#[allow(async_fn_in_trait)]
pub trait SearchSettingsRepo {
    async fn get(&self) -> Result<Option<SearchSettings>, RepoError>;
    /// Stores them for the first time and sets the version to 1; `Duplicate(keys::SETTINGS)`
    /// when they're stored already.
    async fn insert(&self, settings: &mut SearchSettings) -> Result<(), RepoError>;
    /// `StaleVersion` unless the stored settings have `settings`' version.
    async fn update(&self, settings: &mut SearchSettings) -> Result<(), RepoError>;
}

pub mod keys {
    /// There's one settings record per store.
    pub const SETTINGS: &str = "search_settings";
}

pub trait HasSearchQueue {
    type SearchQueue: SearchQueue;
    fn search_queue(&self) -> &Self::SearchQueue;
}

impl<T: HasSearchQueue + ?Sized> HasSearchQueue for &T {
    type SearchQueue = T::SearchQueue;
    fn search_queue(&self) -> &Self::SearchQueue {
        (**self).search_queue()
    }
}

pub trait HasSearchSettings {
    type SearchSettings: SearchSettingsRepo;
    fn search_settings(&self) -> &Self::SearchSettings;
}

impl<T: HasSearchSettings + ?Sized> HasSearchSettings for &T {
    type SearchSettings = T::SearchSettings;
    fn search_settings(&self) -> &Self::SearchSettings {
        (**self).search_settings()
    }
}
