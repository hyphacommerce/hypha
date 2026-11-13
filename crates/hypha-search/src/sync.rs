//! The sync job: works the queue's marks into documents. A plain function of ports, an
//! engine and a clock, like the dispatcher's jobs, so a Workers Cron trigger can call it
//! too.
//!
//! Work per step is bounded and batched: the batch's products load at once (variant
//! marks resolve in one query), stock items load once for every market, prices once
//! per market, the products' translations once per language, and each market's
//! documents, in every language it offers, are written in one `replace`. A product
//! that fails (it can't be loaded, or the engine refuses its documents) fails alone:
//! the rest of the batch is finished, and its marks wait for their lease to run out.

use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use hypha_categories::{CategoryRepo, CategoryTree, HasCategories};
use hypha_channels::{ChannelRepo, HasChannels};
use hypha_core::{
    CategoryId, ChannelId, Clock, ErrorCode, Keyset, Language, MarketId, PageLimit, PageRequest,
    ProductId, Progress, RepoError, SortKey, VariantId,
};
use hypha_custom_fields::{HasCustomFieldDefinitions, HasCustomFieldValues};
use hypha_inventory::{
    HasInventoryItems, HasLocations, InventoryItemRepo, LocationRepo, StockContext,
};
use hypha_markets::{
    CatalogContext, CatalogRepo, HasCatalogs, HasMarkets, HasPriceLists, Market, MarketRepo,
};
use hypha_pricing::load_pricing_for;
use hypha_products::{HasProducts, Product, ProductFilter, ProductRepo, ProductSort};
use hypha_search_index::{
    HasSearchQueue, HasSearchSettings, IndexError, Mark, SearchDocument, SearchIndex, SearchQueue,
    SearchSettings, SearchSettingsRepo, Target,
};
use hypha_translations::{HasTranslations, Overlay, ResourceKind, ResourceRef};
use thiserror::Error;

use crate::{
    build::{InLanguage, Placement},
    build_documents,
    fields::{FacetFields, load_facet_fields},
};

/// Everything the sync job reads: the queue and settings, and what documents are made
/// of. Anything with all of them is `SyncPorts` (the blanket impl below).
pub trait SyncPorts:
    hypha_core::HasClock
    + HasSearchQueue
    + HasSearchSettings
    + HasProducts
    + HasChannels
    + HasCategories
    + HasMarkets
    + HasCatalogs
    + HasPriceLists
    + HasInventoryItems
    + HasLocations
    + HasCustomFieldDefinitions
    + HasCustomFieldValues
    + HasTranslations
{
}

impl<T> SyncPorts for T where
    T: hypha_core::HasClock
        + HasSearchQueue
        + HasSearchSettings
        + HasProducts
        + HasChannels
        + HasCategories
        + HasMarkets
        + HasCatalogs
        + HasPriceLists
        + HasInventoryItems
        + HasLocations
        + HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasTranslations
        + ?Sized
{
}

/// `[jobs.search_sync]` in `hypha.config.toml`, defaults here.
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

impl ErrorCode for SyncError {
    fn code(&self) -> &'static str {
        match self {
            SyncError::Repo(e) => e.code(),
            SyncError::Index(e) => e.code(),
        }
    }
}

/// What a step did: its progress, and the products that failed (left for their lease
/// to run out, then tried again), for the host to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    pub progress: Progress,
    pub failed: Vec<(ProductId, SyncError)>,
}

/// Leases up to `batch` marks and rebuilds what they name: every product for `All`
/// and a market's products for `Market` (by queueing each), a variant's product, a
/// product's documents in every market, in each language the market offers (none once
/// it's gone, inactive or unlisted). `servable` is what the store offers shoppers, its
/// default language first (`Store::offered_languages`): what markets offer out of it,
/// and what translations are read in. `Err` only when the step as a whole couldn't run
/// (the queue, the markets or the settings couldn't be read).
pub async fn sync<S: SyncPorts, I: SearchIndex>(
    ports: &S,
    index: &I,
    clock: &impl Clock,
    settings: &SyncSettings,
    servable: &[Language],
) -> Result<Synced, SyncError> {
    let now = clock.now();
    let marks = ports
        .search_queue()
        .lease(now, now.plus(settings.lease), settings.batch)
        .await?;
    let mut progress = Progress::of(
        u32::try_from(marks.len()).unwrap_or(u32::MAX),
        settings.batch,
    );
    if marks.is_empty() {
        return Ok(Synced {
            progress,
            failed: vec![],
        });
    }

    let mut finished: Vec<Mark> = Vec::new();
    let mut products: BTreeSet<ProductId> = BTreeSet::new();
    let mut variants: Vec<VariantId> = Vec::new();
    let mut categories: Vec<CategoryId> = Vec::new();
    for mark in &marks {
        match mark.target {
            Target::All => {
                queue_every_product(ports).await?;
                finished.push(*mark);
                // It just queued every product: carry on without waiting the interval.
                progress.more = true;
            }
            Target::Market(market) => {
                queue_market_products(ports, market).await?;
                finished.push(*mark);
                progress.more = true;
            }
            Target::Product(id) => {
                products.insert(id);
            }
            Target::Variant(id) => variants.push(id),
            Target::Category(id) => categories.push(id),
        }
    }
    // A moved category's products, and those of every category below it.
    if !categories.is_empty() {
        let tree = CategoryTree::new(ports.categories().list_all().await?);
        let subtree: Vec<CategoryId> = categories
            .iter()
            .flat_map(|c| std::iter::once(*c).chain(tree.descendants(*c)))
            .collect();
        products.extend(ports.categories().products_in(&subtree).await?);
    }
    // A variant that's gone was removed from its product, whose own event rebuilds it.
    let owners = ports.products().ids_of_variants(&variants).await?;
    products.extend(owners.iter().copied());
    let products: Vec<ProductId> = products.into_iter().collect();

    let failed = rebuild(ports, index, &products, servable).await?;
    // Variant marks finish with their product: one that failed may own any of them, so
    // they wait unless every product succeeded.
    let failed_ids: BTreeSet<ProductId> = failed.iter().map(|(id, _)| *id).collect();
    for mark in &marks {
        let done = match mark.target {
            Target::All | Target::Market(_) => false, // finished above
            Target::Product(id) => !failed_ids.contains(&id),
            Target::Variant(_) | Target::Category(_) => failed_ids.is_empty(),
        };
        if done {
            finished.push(*mark);
        }
    }
    let gone = ports.search_queue().finish(&finished).await?;
    if !gone.is_empty() {
        // Another runner rebuilt these after this lease ran out, and this runner may
        // have written older documents over its: rebuild them again.
        ports.search_queue().mark(&gone).await?;
    }
    Ok(Synced { progress, failed })
}

/// Rebuilds `ids`' documents in every market and language; the products that failed,
/// with why.
async fn rebuild<S: SyncPorts, I: SearchIndex>(
    ports: &S,
    index: &I,
    ids: &[ProductId],
    servable: &[Language],
) -> Result<Vec<(ProductId, SyncError)>, SyncError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let (loaded, mut failed) = load_products(ports, ids).await;
    // Deleted products have no documents; ones that failed to load keep theirs.
    let writing: Vec<ProductId> = ids
        .iter()
        .copied()
        .filter(|id| !failed.iter().any(|(f, _)| f == id))
        .collect();
    if writing.is_empty() {
        return Ok(failed);
    }
    let mut sources = Sources::load(ports, loaded, &writing, servable).await?;
    for market in ports.markets().list_all().await? {
        let market_id = market.id();
        let languages = market.offered_languages(servable);
        let documents = sources
            .documents(ports, market, &writing, &languages)
            .await?;
        if let Err(batch_error) = index.replace(market_id, &writing, &documents).await {
            // Find which product the engine refuses: one at a time.
            let mut any_failed = false;
            for id in &writing {
                let own: Vec<SearchDocument> = documents
                    .iter()
                    .filter(|d| d.product == *id)
                    .cloned()
                    .collect();
                if let Err(e) = index.replace(market_id, &[*id], &own).await {
                    any_failed = true;
                    if !failed.iter().any(|(f, _)| f == id) {
                        failed.push((*id, e.into()));
                    }
                }
            }
            if !any_failed {
                // Failed together but not alone: the batch's own trouble (a timeout).
                return Err(batch_error.into());
            }
        }
    }
    Ok(failed)
}

/// What documents are made of, for a set of products, loaded once for every market:
/// the products, their stock items, publications, categories and facet fields, the
/// settings, and their translations into each language as a market first asks for it.
struct Sources {
    loaded: BTreeMap<ProductId, Product>,
    variants: Vec<VariantId>,
    settings: SearchSettings,
    stock: StockContext,
    channels: BTreeMap<ProductId, Vec<ChannelId>>,
    categories: BTreeMap<ProductId, Vec<CategoryId>>,
    fields: BTreeMap<ProductId, FacetFields>,
    /// What the store offers, its own language first: the one nothing is read for.
    servable: Vec<Language>,
    overlays: Vec<(Language, Overlay)>,
}

impl Sources {
    async fn load<S: SyncPorts>(
        ports: &S,
        loaded: BTreeMap<ProductId, Product>,
        ids: &[ProductId],
        servable: &[Language],
    ) -> Result<Self, SyncError> {
        let variants: Vec<VariantId> = loaded
            .values()
            .flat_map(|p| p.variants().iter().map(|v| v.id()))
            .collect();
        let settings = ports.search_settings().get().await?.unwrap_or_default();
        // Items don't depend on the market; only which locations serve it does.
        let items = ports
            .inventory_items()
            .for_variants(&variants)
            .await?
            .into_iter()
            .map(|i| (i.variant(), i))
            .collect();
        let stock = StockContext {
            items,
            locations: vec![],
        };
        // Publications don't depend on the market either.
        let channels = ports
            .channels()
            .channels_of(ids)
            .await?
            .into_iter()
            .collect();
        // Each product's categories with their ancestors, from one tree. Only the ones a
        // storefront shows: a hidden category hides everything below it, so a product only
        // in a hidden subcategory isn't listed under its visible ancestors.
        let tree = CategoryTree::new(ports.categories().list_all().await?);
        let categories = ports
            .categories()
            .categories_of(ids)
            .await?
            .into_iter()
            .map(|(product, direct)| {
                let all: BTreeSet<CategoryId> = direct
                    .iter()
                    .filter(|c| tree.is_shown(**c))
                    .flat_map(|c| std::iter::once(*c).chain(tree.ancestors(*c)))
                    .collect();
                (product, all.into_iter().collect())
            })
            .collect();
        // Custom field values a storefront filters by.
        let products: Vec<&Product> = loaded.values().collect();
        let fields = load_facet_fields(ports, &products).await?;
        Ok(Self {
            loaded,
            variants,
            settings,
            stock,
            channels,
            categories,
            fields,
            servable: servable.to_vec(),
            overlays: Vec::new(),
        })
    }

    /// Reads the products' translations into each of `languages` not read yet: one read
    /// per language for the whole batch, none for the store's own.
    async fn load_overlays<S: SyncPorts>(
        &mut self,
        ports: &S,
        languages: &[Language],
    ) -> Result<(), SyncError> {
        let resources: Vec<ResourceRef> = self
            .loaded
            .keys()
            .map(|id| ResourceRef::new(ResourceKind::Product, id.as_uuid()))
            .collect();
        for language in languages {
            if self.overlays.iter().any(|(l, _)| l == language) {
                continue;
            }
            let overlay = if self.servable.first() == Some(language) {
                Overlay::none()
            } else {
                Overlay::load(ports, language, &self.servable, &resources).await?
            };
            self.overlays.push((language.clone(), overlay));
        }
        Ok(())
    }

    /// `ids`' documents in `market`, priced and stocked there, in each of `languages`.
    async fn documents<S: SyncPorts>(
        &mut self,
        ports: &S,
        market: Market,
        ids: &[ProductId],
        languages: &[Language],
    ) -> Result<Vec<SearchDocument>, SyncError> {
        let market_id = market.id();
        // The market as the caller has it, not read again. Every shopper shares a
        // document, so it carries public prices: no customer groups.
        let pricing = load_pricing_for(ports, market, ids, &self.variants, &[]).await?;
        self.stock.locations = ports.locations().serving(market_id).await?;
        self.load_overlays(ports, languages).await?;
        let none: &[ChannelId] = &[];
        let no_categories: &[CategoryId] = &[];
        let no_fields = FacetFields::default();
        let mut documents = Vec::new();
        for (language, overlay) in self.overlays.iter().filter(|(l, _)| languages.contains(l)) {
            for product in ids.iter().filter_map(|id| self.loaded.get(id)) {
                let placement = Placement {
                    channels: self.channels.get(&product.id()).map_or(none, Vec::as_slice),
                    categories: self
                        .categories
                        .get(&product.id())
                        .map_or(no_categories, Vec::as_slice),
                    fields: self.fields.get(&product.id()).unwrap_or(&no_fields),
                };
                documents.extend(build_documents(
                    product,
                    &pricing,
                    &self.stock,
                    &self.settings,
                    placement,
                    InLanguage { language, overlay },
                ));
            }
        }
        Ok(documents)
    }
}

/// `ids`' documents in `market` and `language` as the sync job would build them now:
/// priced and stocked there, none for a product that's gone, inactive or not in the
/// market's catalog. `servable` as `sync` takes it. For hits an app found: Hypha shows its
/// own prices, stock and text, never an app's.
pub async fn documents_for<S: SyncPorts>(
    ports: &S,
    market: Market,
    ids: &[ProductId],
    language: &Language,
    servable: &[Language],
) -> Result<Vec<SearchDocument>, SyncError> {
    let loaded: BTreeMap<ProductId, Product> = ports
        .products()
        .get_many(ids)
        .await?
        .into_iter()
        .map(|p| (p.id(), p))
        .collect();
    let mut sources = Sources::load(ports, loaded, ids, servable).await?;
    sources
        .documents(ports, market, ids, std::slice::from_ref(language))
        .await
}

/// `ids`' products, in one query; if that fails (one can't be restored, say), one at a
/// time, so the others still load. The ones that failed, with why.
async fn load_products<S: HasProducts>(
    ports: &S,
    ids: &[ProductId],
) -> (BTreeMap<ProductId, Product>, Vec<(ProductId, SyncError)>) {
    let by_id = |products: Vec<Product>| products.into_iter().map(|p| (p.id(), p)).collect();
    match ports.products().get_many(ids).await {
        Ok(products) => (by_id(products), vec![]),
        Err(_) => {
            let mut loaded = BTreeMap::new();
            let mut failed = Vec::new();
            for id in ids {
                match ports.products().get(*id).await {
                    Ok(Some(product)) => {
                        loaded.insert(*id, product);
                    }
                    Ok(None) => {}
                    Err(e) => failed.push((*id, e.into())),
                }
            }
            (loaded, failed)
        }
    }
}

/// Marks the products `market`'s catalog sells, a page at a time. A product it stopped
/// selling was rebuilt by the catalog's own event.
async fn queue_market_products<S: SyncPorts>(ports: &S, market: MarketId) -> Result<(), RepoError> {
    let Some(catalog) = ports
        .catalogs()
        .get_by_context(CatalogContext::Market(market))
        .await?
    else {
        return Ok(());
    };
    let mut after = None;
    loop {
        let page = ports
            .catalogs()
            .product_page(
                catalog.id(),
                PageRequest {
                    after,
                    limit: PageLimit::new(PageLimit::MAX)
                        .unwrap_or_else(|_| unreachable!("MAX is a valid limit")),
                },
            )
            .await?;
        let targets: Vec<Target> = page.items.iter().map(|p| Target::Product(*p)).collect();
        ports.search_queue().mark(&targets).await?;
        match page.items.last() {
            Some(last) if page.has_more => after = Some(*last),
            _ => return Ok(()),
        }
    }
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
                ProductSort::Oldest,
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
            Some(last) if page.has_more => {
                after = Some(Keyset {
                    key: SortKey::Id,
                    id: last.id,
                });
            }
            _ => return Ok(()),
        }
    }
}
