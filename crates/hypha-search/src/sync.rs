//! The sync job (design note 0019, section 5): works the queue's marks into documents.
//! A plain function of ports, an engine and a clock, like the dispatcher's jobs, so a
//! Workers Cron trigger can call it too.

use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use hypha_core::{Clock, PageLimit, PageRequest, ProductId, Progress, RepoError, VariantId};
use hypha_inventory::{HasInventoryItems, HasLocations, StockContext};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists, MarketRepo};
use hypha_pricing::load_pricing;
use hypha_products::{HasProducts, Product, ProductFilter, ProductRepo};
use thiserror::Error;

use crate::{
    HasSearchQueue, HasSearchSettings, IndexError, SearchIndex, SearchQueue, SearchSettingsRepo,
    Target, build_documents,
};

/// Everything the sync job reads: the queue and settings, and what documents are made
/// of. Anything with all of them is `SyncPorts` (the blanket impl below).
pub trait SyncPorts:
    HasSearchQueue
    + HasSearchSettings
    + HasProducts
    + HasMarkets
    + HasCatalogs
    + HasPriceLists
    + HasInventoryItems
    + HasLocations
{
}

impl<T> SyncPorts for T where
    T: HasSearchQueue
        + HasSearchSettings
        + HasProducts
        + HasMarkets
        + HasCatalogs
        + HasPriceLists
        + HasInventoryItems
        + HasLocations
        + ?Sized
{
}

/// `[jobs.search_sync]` in `hypha.config.toml` (design note 0017), defaults here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSettings {
    /// Marks worked per call: each is a product rebuilt in every market.
    pub batch: u32,
    /// How long a mark is leased: past this, another runner may take it.
    pub lease: Duration,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            batch: 50,
            lease: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SyncError {
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Leases up to `batch` marks and rebuilds what they name: every product for `All`
/// (by queueing each), a variant's product, a product's documents in every market (none
/// once it's gone, inactive or unlisted). The marks are finished only if all of it
/// worked; otherwise their lease runs out and a later run tries again.
pub async fn sync<S: SyncPorts, I: SearchIndex>(
    ports: &S,
    index: &I,
    clock: &impl Clock,
    settings: &SyncSettings,
) -> Result<Progress, SyncError> {
    let now = clock.now();
    let marks = ports
        .search_queue()
        .lease(now, now.plus(settings.lease), settings.batch)
        .await?;
    if marks.is_empty() {
        return Ok(Progress::of(0, settings.batch));
    }

    let mut products: BTreeSet<ProductId> = BTreeSet::new();
    for mark in &marks {
        match mark.target {
            Target::All => queue_every_product(ports).await?,
            Target::Product(id) => {
                products.insert(id);
            }
            Target::Variant(variant) => {
                // A variant that's gone was removed from its product, whose own event
                // rebuilds it.
                if let Some(product) = ports.products().get_by_variant(variant).await? {
                    products.insert(product.id());
                }
            }
        }
    }
    rebuild(ports, index, &products.into_iter().collect::<Vec<_>>()).await?;
    ports.search_queue().finish(&marks).await?;
    Ok(Progress::of(marks.len() as u32, settings.batch))
}

/// Rebuilds `ids`' documents in every market, loading prices and stock once per market
/// for all of them.
async fn rebuild<S: SyncPorts, I: SearchIndex>(
    ports: &S,
    index: &I,
    ids: &[ProductId],
) -> Result<(), SyncError> {
    if ids.is_empty() {
        return Ok(());
    }
    let loaded: BTreeMap<ProductId, Product> = ports
        .products()
        .get_many(ids)
        .await?
        .into_iter()
        .map(|p| (p.id(), p))
        .collect();
    let variants: Vec<VariantId> = loaded
        .values()
        .flat_map(|p| p.variants().iter().map(|v| v.id()))
        .collect();
    let search_settings = ports.search_settings().get().await?.unwrap_or_default();

    for market in ports.markets().list_all().await? {
        let pricing = load_pricing(ports, market.id(), ids, &variants).await?;
        let stock = StockContext::load(ports, market.id(), &variants).await?;
        for id in ids {
            let documents = match (loaded.get(id), &pricing) {
                (Some(product), Some(pricing)) => {
                    build_documents(product, pricing, &stock, &search_settings)
                }
                // Deleted, or the market went while we loaded.
                _ => Vec::new(),
            };
            index.replace(market.id(), *id, &documents).await?;
        }
    }
    Ok(())
}

/// Marks every product, a page at a time.
async fn queue_every_product<S: SyncPorts>(ports: &S) -> Result<(), RepoError> {
    let every = ProductFilter::default();
    let mut after = None;
    loop {
        let page = ports
            .products()
            .list(
                &every,
                PageRequest {
                    after,
                    limit: PageLimit::new(PageLimit::MAX)
                        .unwrap_or_else(|_| unreachable!("MAX is a valid limit")),
                },
            )
            .await?;
        let targets: Vec<Target> = page.items.iter().map(|p| Target::Product(p.id)).collect();
        ports.search_queue().mark(&targets).await?;
        match page.items.last() {
            Some(last) if page.has_more => after = Some(last.id),
            _ => return Ok(()),
        }
    }
}

/// Queues a rebuild of everything (design note 0019, section 5): after a new engine or
/// a lost index. The host's reindex route calls it through `reindex`.
pub async fn queue_all<S: HasSearchQueue>(ports: &S) -> Result<(), RepoError> {
    ports.search_queue().mark(&[Target::All]).await
}
