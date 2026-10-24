//! Search over a store in memory: events become marks, the sync job turns marks into
//! documents priced and stocked per market, and the commands search them and keep the
//! merchant's settings.

use std::cell::Cell;

use hypha_core::{
    Actor, AuthError, BasisPoints, CatalogId, CountryCode, Ctx, Currency, EventHandler,
    InventoryItemId, LocationId, MarketId, Money, OptionId, OptionValueId, Price, PriceListId,
    ProductId, Scope, UserId, VariantId,
};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_products::{
    Handle, NewVariant, OptionValue, Product, ProductOption, ProductRepo, ProductStatus,
};
use hypha_search::{CommandError, SearchIndexer, SearchInput, SyncSettings, search, sync};
use hypha_search_index::{
    CommandError as IndexError, FacetKey, Group, SearchDocument, SearchError, SearchStatus, Target,
    UpdateSearchSettings, get_search_settings, reindex, search_status, update_search_settings,
};
use hypha_testkit::{FixedClock, InMemoryPorts, InMemorySearchIndex};
use pollster::block_on;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn hr_market() -> MarketId {
    MarketId::from_uuid(id(1000))
}
fn us_market() -> MarketId {
    MarketId::from_uuid(id(1001))
}
fn shirt() -> ProductId {
    ProductId::from_uuid(id(1100))
}
fn mug() -> ProductId {
    ProductId::from_uuid(id(1101))
}
fn draft() -> ProductId {
    ProductId::from_uuid(id(1102))
}
/// Shirt: Red/S, Red/M, Blue/S. Mug: its only variant.
fn red_s() -> VariantId {
    VariantId::from_uuid(id(1200))
}
fn red_m() -> VariantId {
    VariantId::from_uuid(id(1201))
}
fn blue_s() -> VariantId {
    VariantId::from_uuid(id(1202))
}
fn mug_v() -> VariantId {
    VariantId::from_uuid(id(1203))
}
fn warehouse() -> LocationId {
    LocationId::from_uuid(id(1300))
}
fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}
fn price(amount: i64) -> Price {
    Price::new(eur(amount)).unwrap()
}

struct World {
    ports: InMemoryPorts,
    index: InMemorySearchIndex,
    clock: FixedClock,
    /// How many outbox events `deliver` has handled.
    delivered: Cell<usize>,
}

impl World {
    /// Croatia (EUR, a −10% price list with a fixed 1000 for Blue/S) sells the shirt,
    /// the mug (none in stock) and a draft; the US (USD) lists the shirt, whose EUR
    /// prices don't resolve there.
    fn new() -> Self {
        let w = Self {
            ports: InMemoryPorts::default(),
            index: InMemorySearchIndex::default(),
            clock: FixedClock::default(),
            delivered: Cell::new(0),
        };
        let p = &w.ports;
        for (market, country, currency) in [
            (hr_market(), "HR", Currency::EUR),
            (us_market(), "US", Currency::USD),
        ] {
            let mut m = Market::new(
                market,
                country.to_string(),
                vec![CountryCode::new(country).unwrap()],
                currency,
            )
            .unwrap();
            block_on(p.markets.insert(&mut m)).unwrap();
        }

        let value =
            |n, label: &str| OptionValue::new(OptionValueId::from_uuid(id(n)), label.to_string());
        let color = ProductOption::new(
            OptionId::from_uuid(id(1400)),
            "Color".to_string(),
            vec![value(1401, "Red"), value(1402, "Blue")],
        );
        let size = ProductOption::new(
            OptionId::from_uuid(id(1410)),
            "Size".to_string(),
            vec![value(1411, "S"), value(1412, "M")],
        );
        let variant = |v, c, s, amount| NewVariant {
            id: v,
            sku: None,
            price: price(amount),
            selections: vec![
                OptionValueId::from_uuid(id(c)),
                OptionValueId::from_uuid(id(s)),
            ],
        };
        let mut shirt_p = Product::new_configurable(
            shirt(),
            Handle::new("linen-shirt").unwrap(),
            "Linen Shirt".to_string(),
            vec![color, size],
            variant(red_s(), 1401, 1411, 3000),
        )
        .unwrap();
        shirt_p
            .add_variant(variant(red_m(), 1401, 1412, 3200))
            .unwrap();
        shirt_p
            .add_variant(variant(blue_s(), 1402, 1411, 2800))
            .unwrap();
        shirt_p
            .set_description(Some("<p>Breathable <em>linen</em></p>"))
            .unwrap();
        shirt_p.set_vendor(Some("Acme".to_string())).unwrap();
        shirt_p.set_tags(vec!["Summer".to_string()]).unwrap();
        shirt_p.set_status(ProductStatus::Active).unwrap();
        block_on(p.products.insert(&mut shirt_p)).unwrap();

        for (product, variant, handle, active) in [
            (mug(), mug_v(), "mug", true),
            (draft(), VariantId::from_uuid(id(1204)), "draft", false),
        ] {
            let mut simple = Product::new_simple(
                product,
                Handle::new(handle).unwrap(),
                handle.to_string(),
                variant,
                price(1000),
            )
            .unwrap();
            if active {
                simple.set_status(ProductStatus::Active).unwrap();
            }
            block_on(p.products.insert(&mut simple)).unwrap();
        }

        let list = PriceListId::from_uuid(id(1500));
        let mut prices = PriceList::new(
            list,
            hypha_markets::PriceListDraft::new("Croatia", Currency::EUR)
                .adjusted(BasisPoints::new(-1000).unwrap()),
        )
        .unwrap();
        block_on(p.price_lists.insert(&mut prices)).unwrap();
        block_on(p.price_lists.set_fixed_prices(
            list,
            &[(blue_s(), hypha_markets::FixedPrice::plain(price(1000)))],
        ))
        .unwrap();
        for (n, market, price_list, products) in [
            (1600, hr_market(), Some(list), vec![shirt(), mug(), draft()]),
            (1601, us_market(), None, vec![shirt()]),
        ] {
            let catalog_id = CatalogId::from_uuid(id(n));
            let mut catalog = Catalog::new(catalog_id, CatalogContext::Market(market), price_list);
            block_on(p.catalogs.insert(&mut catalog)).unwrap();
            block_on(p.catalogs.add_products(catalog_id, &products)).unwrap();
        }

        let mut location =
            Location::new(warehouse(), "Zagreb".to_string(), vec![hr_market()], 0).unwrap();
        block_on(p.locations.insert(&mut location)).unwrap();
        let mut item =
            InventoryItem::new(InventoryItemId::from_uuid(id(1700)), mug_v(), true, false);
        item.set_on_hand(warehouse(), 0, None).unwrap();
        block_on(p.inventory_items.insert(&mut item)).unwrap();
        w
    }

    /// Hands the outbox's new events to the handler, as the dispatcher would.
    fn deliver(&self) {
        let events = self.ports.outbox.events();
        for event in &events[self.delivered.get()..] {
            block_on(EventHandler::handle(&SearchIndexer, &self.ports, event)).unwrap();
        }
        self.delivered.set(events.len());
    }

    /// Delivers, then syncs until the queue is empty.
    fn settle(&self) {
        self.deliver();
        while block_on(sync(
            &self.ports,
            &self.index,
            &self.clock,
            &SyncSettings::default(),
        ))
        .unwrap()
        .progress
        .more
            || !self.ports.search_queue.targets().is_empty()
        {}
    }

    fn documents(&self, market: MarketId) -> Vec<SearchDocument> {
        let mut docs: Vec<SearchDocument> = self
            .index
            .documents()
            .into_iter()
            .filter(|d| d.market == market)
            .collect();
        docs.sort_by_key(|d| d.variant);
        docs
    }

    fn search(&self, input: SearchInput) -> Result<hypha_search::Found, CommandError> {
        block_on(search(&Ctx::system(), &self.ports, &self.index, input))
    }
}

fn input(country: &str) -> SearchInput {
    SearchInput {
        country: country.to_string(),
        limit: 10,
        ..SearchInput::default()
    }
}

// ---------- documents and sync ----------

#[test]
fn a_store_syncs_into_documents_priced_and_stocked_per_market() {
    let w = World::new();
    w.settle();

    let hr = w.documents(hr_market());
    let variants: Vec<VariantId> = hr.iter().map(|d| d.variant).collect();
    // The draft has none; the shirt and the mug do.
    assert_eq!(variants, [red_s(), red_m(), blue_s(), mug_v()]);
    let prices: Vec<i64> = hr.iter().map(|d| d.price.amount()).collect();
    // −10% on the shirt's base prices, Blue/S fixed at 1000, the mug −10%.
    assert_eq!(prices, [2700, 2880, 1000, 900]);
    let shirt_doc = &hr[0];
    assert_eq!(
        (
            shirt_doc.title.as_str(),
            shirt_doc.variant_title.as_deref(),
            shirt_doc.description.as_str(),
            shirt_doc.vendor.as_deref(),
            &shirt_doc.tags[..],
            &shirt_doc.options[..],
        ),
        (
            "Linen Shirt",
            Some("Red / S"),
            "Breathable linen",
            Some("Acme"),
            &["Summer".to_string()][..],
            &[
                ("Color".to_string(), "Red".to_string()),
                ("Size".to_string(), "S".to_string())
            ][..],
        )
    );
    assert_eq!(shirt_doc.listing, shirt().to_string(), "no splits yet");
    assert_eq!(
        (
            shirt_doc.product_price.min.amount(),
            shirt_doc.product_price.max.amount()
        ),
        (1000, 2880)
    );
    // Untracked shirts sell; the mug has none in stock.
    let available: Vec<bool> = hr.iter().map(|d| d.available).collect();
    assert_eq!(available, [true, true, true, false]);
    // EUR prices don't resolve in a USD market until exchange rates exist.
    assert!(w.documents(us_market()).is_empty());
}

/// An engine that refuses one product's documents, as an external engine might.
struct Refusing<'a> {
    inner: &'a InMemorySearchIndex,
    refuse: ProductId,
}

impl hypha_search_index::SearchIndex for Refusing<'_> {
    async fn replace(
        &self,
        market: MarketId,
        products: &[ProductId],
        documents: &[SearchDocument],
    ) -> Result<(), hypha_search_index::IndexError> {
        if products.contains(&self.refuse) {
            return Err(hypha_search_index::IndexError("too large".to_string()));
        }
        self.inner.replace(market, products, documents).await
    }

    async fn search(
        &self,
        query: &hypha_search_index::SearchQuery,
    ) -> Result<hypha_search_index::SearchResults, hypha_search_index::IndexError> {
        self.inner.search(query).await
    }

    async fn option_names(
        &self,
        market: MarketId,
        channel: Option<hypha_core::ChannelId>,
    ) -> Result<Vec<String>, hypha_search_index::IndexError> {
        self.inner.option_names(market, channel).await
    }

    async fn count(&self) -> Result<u64, hypha_search_index::IndexError> {
        self.inner.count().await
    }
}

#[test]
fn a_product_the_engine_refuses_fails_alone() {
    let w = World::new();
    w.deliver();
    let refusing = Refusing {
        inner: &w.index,
        refuse: mug(),
    };
    // Until nothing more can be done: the mug's marks are leased after each failure.
    let mut failed = Vec::new();
    loop {
        let synced = block_on(sync(
            &w.ports,
            &refusing,
            &w.clock,
            &SyncSettings::default(),
        ))
        .unwrap();
        failed.extend(synced.failed.iter().map(|(id, _)| *id));
        if synced.progress.done == 0 {
            break;
        }
    }
    failed.dedup();
    assert_eq!(failed, [mug()]);
    // The shirt is in; the mug's marks (its product, its stock) wait for their lease.
    let variants: Vec<VariantId> = w.documents(hr_market()).iter().map(|d| d.variant).collect();
    assert_eq!(variants, [red_s(), red_m(), blue_s()]);
    let mut waiting = w.ports.search_queue.targets();
    waiting.sort();
    assert_eq!(waiting, [Target::Product(mug()), Target::Variant(mug_v())]);
}

#[test]
fn a_product_leaving_active_leaves_the_index() {
    let w = World::new();
    w.settle();
    let mut p = block_on(w.ports.products.get(shirt())).unwrap().unwrap();
    p.set_status(ProductStatus::Archived).unwrap();
    block_on(w.ports.products.update(&mut p)).unwrap();
    w.settle();
    let variants: Vec<VariantId> = w.documents(hr_market()).iter().map(|d| d.variant).collect();
    assert_eq!(variants, [mug_v()]);
}

#[test]
fn a_deleted_product_leaves_the_index() {
    use hypha_core::HasBatch;
    use hypha_products::StageProducts;
    let w = World::new();
    w.settle();
    let mut p = block_on(w.ports.products.get(shirt())).unwrap().unwrap();
    p.delete();
    let mut batch = w.ports.batch();
    batch.delete_product(&mut p);
    block_on(w.ports.commit(batch)).unwrap();
    w.deliver();
    assert_eq!(w.ports.search_queue.targets(), [Target::Product(shirt())]);
    w.settle();
    let variants: Vec<VariantId> = w.documents(hr_market()).iter().map(|d| d.variant).collect();
    assert_eq!(variants, [mug_v()]);
}

#[test]
fn stock_arriving_makes_a_variant_available() {
    let w = World::new();
    w.settle();
    let mut item = block_on(w.ports.inventory_items.get_by_variant(mug_v()))
        .unwrap()
        .unwrap();
    item.set_on_hand(warehouse(), 5, None).unwrap();
    block_on(w.ports.inventory_items.update(&mut item)).unwrap();
    w.deliver();
    assert_eq!(w.ports.search_queue.targets(), [Target::Variant(mug_v())]);
    w.settle();
    let mug_doc = w.documents(hr_market()).pop().unwrap();
    assert!(mug_doc.available);
}

#[test]
fn many_changes_to_one_product_are_one_rebuild() {
    let w = World::new();
    w.settle();
    let mut p = block_on(w.ports.products.get(shirt())).unwrap().unwrap();
    for title in ["Shirt A", "Shirt B", "Shirt C"] {
        p.rename(title).unwrap();
        block_on(w.ports.products.update(&mut p)).unwrap();
    }
    w.deliver();
    assert_eq!(w.ports.search_queue.targets(), [Target::Product(shirt())]);
    w.settle();
    assert_eq!(w.documents(hr_market())[0].title, "Shirt C");
}

#[test]
fn splitting_by_an_option_rebuilds_every_listing() {
    let w = World::new();
    w.settle();
    block_on(update_search_settings(
        &Ctx::system(),
        &w.ports,
        &w.ports,
        UpdateSearchSettings {
            split_by: Some(vec!["color".to_string()]),
            ..UpdateSearchSettings::default()
        },
    ))
    .unwrap();
    w.deliver();
    assert_eq!(w.ports.search_queue.targets(), [Target::All]);
    w.settle();
    let listings: Vec<String> = w
        .documents(hr_market())
        .iter()
        .map(|d| d.listing.clone())
        .collect();
    assert_eq!(
        listings,
        [
            format!("{}|color=Red", shirt()),
            format!("{}|color=Red", shirt()),
            format!("{}|color=Blue", shirt()),
            mug().to_string(),
        ]
    );
    // Red's price range is its own two variants'.
    let red = &w.documents(hr_market())[0];
    assert_eq!(
        (
            red.listing_price.min.amount(),
            red.listing_price.max.amount()
        ),
        (2700, 2880)
    );
}

#[test]
fn changing_only_the_facets_rebuilds_nothing() {
    let w = World::new();
    w.settle();
    block_on(update_search_settings(
        &Ctx::system(),
        &w.ports,
        &w.ports,
        UpdateSearchSettings {
            facets: Some(vec![("vendor".to_string(), Some("Brand".to_string()))]),
            ..UpdateSearchSettings::default()
        },
    ))
    .unwrap();
    w.deliver();
    assert!(w.ports.search_queue.targets().is_empty());
}

#[test]
fn a_full_rebuild_says_there_is_more_to_do_at_once() {
    let w = World::new();
    w.settle();
    block_on(reindex(&Ctx::system(), &w.ports)).unwrap();
    let synced = block_on(sync(&w.ports, &w.index, &w.clock, &SyncSettings::default())).unwrap();
    assert_eq!((synced.progress.done, synced.progress.more), (1, true));
    assert_eq!(
        w.ports.search_queue.targets().len(),
        3,
        "every product queued"
    );
}

#[test]
fn reindex_queues_everything_and_status_counts() {
    let w = World::new();
    w.settle();
    let ctx = Ctx::system();
    block_on(reindex(&ctx, &w.ports)).unwrap();
    assert_eq!(
        block_on(search_status(&ctx, &w.ports, &w.index)),
        Ok(SearchStatus {
            queued: 1,
            documents: 4
        })
    );
    w.settle();
    assert_eq!(
        block_on(search_status(&ctx, &w.ports, &w.index)).map(|s| s.queued),
        Ok(0)
    );
}

// ---------- the search command ----------

#[test]
fn search_finds_listings_in_the_shoppers_market() {
    let w = World::new();
    w.settle();
    let found = w
        .search(SearchInput {
            q: Some("linen".to_string()),
            ..input("HR")
        })
        .unwrap();
    assert_eq!(found.market.id(), hr_market());
    assert_eq!(found.results.total, 1);
    assert_eq!(found.results.hits[0].matched, 3);

    let by_variant = w
        .search(SearchInput {
            filters: vec!["option.Color:Red".to_string()],
            group: Group::Variant,
            ..input("HR")
        })
        .unwrap();
    assert_eq!(by_variant.results.total, 2);
}

#[test]
fn default_facets_are_type_vendor_the_markets_options_and_tags() {
    let w = World::new();
    w.settle();
    let found = w.search(input("HR")).unwrap();
    let keys: Vec<FacetKey> = found.results.facets.iter().map(|f| f.key.clone()).collect();
    assert_eq!(
        keys,
        [
            FacetKey::ProductType,
            FacetKey::Vendor,
            FacetKey::Option("color".to_string()),
            FacetKey::Option("size".to_string()),
            FacetKey::Tag,
        ]
    );
    assert!(found.labels.is_empty());
}

#[test]
fn the_merchants_facets_and_labels_replace_the_defaults() {
    let w = World::new();
    w.settle();
    block_on(update_search_settings(
        &Ctx::system(),
        &w.ports,
        &w.ports,
        UpdateSearchSettings {
            facets: Some(vec![
                ("option.Size".to_string(), Some("Size".to_string())),
                ("tag".to_string(), None),
            ]),
            ..UpdateSearchSettings::default()
        },
    ))
    .unwrap();
    let found = w.search(input("HR")).unwrap();
    let keys: Vec<FacetKey> = found.results.facets.iter().map(|f| f.key.clone()).collect();
    assert_eq!(keys, [FacetKey::Option("size".to_string()), FacetKey::Tag]);
    assert_eq!(
        found.labels.get(&FacetKey::Option("size".to_string())),
        Some(&"Size".to_string())
    );
}

#[test]
fn a_country_no_market_sells_to_is_refused() {
    let w = World::new();
    let de = CountryCode::new("DE").unwrap();
    assert_eq!(
        w.search(input("DE")).map(|_| ()),
        Err(CommandError::NoMarketForCountry(de))
    );
}

#[test]
fn bad_searches_say_what_is_wrong() {
    let w = World::new();
    let with = |f: &dyn Fn(&mut SearchInput)| {
        let mut i = input("HR");
        f(&mut i);
        w.search(i).map(|_| ())
    };
    let err = |e: SearchError| Err(CommandError::Search(e));
    assert_eq!(
        with(&|i| i.filters = vec!["Red".to_string()]),
        err(SearchError::InvalidFilter("Red".to_string()))
    );
    assert_eq!(
        with(&|i| i.filters = vec!["color:Red".to_string()]),
        err(SearchError::UnknownFacet("color".to_string()))
    );
    assert_eq!(
        with(&|i| {
            i.price_min = Some(10);
            i.price_max = Some(9);
        }),
        err(SearchError::InvalidPriceRange)
    );
    assert_eq!(
        with(&|i| i.limit = 0),
        err(SearchError::InvalidLimit { max: 100 })
    );
    assert_eq!(
        with(&|i| i.offset = 1000),
        err(SearchError::TooDeep { max: 1000 })
    );
    // The last page is cut short at the limit instead of refused.
    assert_eq!(
        with(&|i| {
            i.offset = 999;
            i.limit = 10;
        }),
        Ok(())
    );
}

#[test]
fn searching_needs_products_read_and_settings_need_write() {
    let w = World::new();
    let ctx = Ctx::new(
        Actor::User(UserId::from_uuid(id(9))),
        vec![Scope::OrdersRead],
    );
    assert_eq!(
        block_on(search(&ctx, &w.ports, &w.index, input("HR"))).map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsRead
        )))
    );
    let missing = |scope| Err(IndexError::Auth(AuthError::MissingScope(scope)));
    assert_eq!(
        block_on(get_search_settings(&ctx, &w.ports)).map(|_| ()),
        missing(Scope::ProductsRead)
    );
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(id(9))),
        vec![Scope::ProductsRead],
    );
    assert_eq!(
        block_on(update_search_settings(
            &reader,
            &w.ports,
            &w.ports,
            UpdateSearchSettings::default()
        ))
        .map(|_| ()),
        missing(Scope::ProductsWrite)
    );
    assert_eq!(
        block_on(reindex(&reader, &w.ports)),
        missing(Scope::ProductsWrite)
    );
}

#[test]
fn settings_are_stored_on_first_save_and_updated_after() {
    let w = World::new();
    let ctx = Ctx::system();
    assert_eq!(
        block_on(get_search_settings(&ctx, &w.ports)).map(|s| s.split_by().to_vec()),
        Ok(vec![])
    );
    let save = |names: &[&str]| {
        block_on(update_search_settings(
            &ctx,
            &w.ports,
            &w.ports,
            UpdateSearchSettings {
                split_by: Some(names.iter().map(|n| n.to_string()).collect()),
                ..UpdateSearchSettings::default()
            },
        ))
    };
    assert!(save(&["Color"]).is_ok());
    assert!(save(&["Color", "Size"]).is_ok());
    let stored = block_on(get_search_settings(&ctx, &w.ports)).unwrap();
    assert_eq!(stored.split_by(), ["Color", "Size"]);
    let changes = w
        .ports
        .outbox
        .events()
        .iter()
        .filter(|e| e.event_type == "search_settings.changed")
        .count();
    assert_eq!(changes, 2);
    assert_eq!(
        save(&["a", "b", "c", "d"]).map(|_| ()),
        Err(IndexError::Search(SearchError::TooManySplits { max: 3 }))
    );
}

// ---------- the handler ----------

fn event(event_type: &str, data: serde_json::Value) -> hypha_core::StoredEvent {
    hypha_core::StoredEvent {
        seq: 1,
        id: id(1),
        event_type: event_type.to_string(),
        version: 1,
        subject_kind: "any".to_string(),
        subject_id: id(2),
        subject_version: None,
        actor: Actor::System,
        occurred_at: hypha_core::Timestamp::from_unix_micros(0),
        data,
    }
}

#[test]
fn a_storefront_searches_only_what_its_channel_publishes() {
    let w = World::new();
    // Publishes every product before the first sync builds their documents.
    let storefront = w.ports.storefront();
    let channel = storefront.channel().unwrap();
    w.settle();
    assert!(
        w.documents(hr_market())
            .iter()
            .all(|d| d.channels == [channel])
    );
    let linen = || SearchInput {
        q: Some("linen".to_string()),
        ..input("HR")
    };
    let found = block_on(search(&storefront, &w.ports, &w.index, linen())).unwrap();
    assert_eq!(found.results.total, 1);

    block_on(hypha_channels::unpublish_products(
        &Ctx::system(),
        &w.ports,
        channel,
        vec![shirt()],
    ))
    .unwrap();
    w.settle();
    let found = block_on(search(&storefront, &w.ports, &w.index, linen())).unwrap();
    assert_eq!(found.results.total, 0);
    // Staff search every channel.
    assert_eq!(w.search(linen()).unwrap().results.total, 1);

    // Default facets are the channel's own: the shirt's options went with it.
    let options = |found: &hypha_search::Found| -> Vec<FacetKey> {
        let keys = found.results.facets.iter().map(|f| f.key.clone());
        keys.filter(|k| matches!(k, FacetKey::Option(_))).collect()
    };
    let everything = block_on(search(&storefront, &w.ports, &w.index, input("HR"))).unwrap();
    assert_eq!(options(&everything), vec![]);
    assert!(!options(&w.search(input("HR")).unwrap()).is_empty());
}

#[test]
fn each_event_marks_what_it_changes() {
    use hypha_search::handler::targets;
    let product = serde_json::json!({ "product": shirt() });
    assert_eq!(
        targets(&event("product.updated", product)),
        Ok(vec![Target::Product(shirt())])
    );
    let products = serde_json::json!({ "catalog": "cat_x", "products": [shirt(), mug()] });
    assert_eq!(
        targets(&event("catalog.products_removed", products)),
        Ok(vec![Target::Product(shirt()), Target::Product(mug())])
    );
    let published = serde_json::json!({ "channel": "chan_x", "products": [mug()] });
    assert_eq!(
        targets(&event("channel.products_unpublished", published)),
        Ok(vec![Target::Product(mug())])
    );
    let variants = serde_json::json!({ "price_list": "pl_x", "variants": [red_s()] });
    assert_eq!(
        targets(&event("price_list.prices_set", variants)),
        Ok(vec![Target::Variant(red_s())])
    );
    assert_eq!(
        targets(&event("market.changed", serde_json::json!({}))),
        Ok(vec![Target::All])
    );
    let renamed = serde_json::json!({ "location": "loc_x", "fields": ["name"] });
    assert_eq!(targets(&event("location.changed", renamed)), Ok(vec![]));
    let moved = serde_json::json!({ "location": "loc_x", "fields": ["markets", "name"] });
    assert_eq!(
        targets(&event("location.changed", moved)),
        Ok(vec![Target::All])
    );
    let facets_only = serde_json::json!({ "fields": ["facets"] });
    assert_eq!(
        targets(&event("search_settings.changed", facets_only)),
        Ok(vec![])
    );
}

#[test]
fn events_search_ignores_mark_nothing() {
    let w = World::new();
    let placed = event("order.placed", serde_json::json!({}));
    assert!(!EventHandler::<InMemoryPorts>::wants(
        &SearchIndexer,
        &placed.event_type
    ));
    block_on(EventHandler::handle(&SearchIndexer, &w.ports, &placed)).unwrap();
    assert!(w.ports.search_queue.targets().is_empty());
}

#[test]
fn a_malformed_payload_fails_the_delivery() {
    let w = World::new();
    let bad = event(
        "inventory_item.changed",
        serde_json::json!({ "variant": 7 }),
    );
    assert!(block_on(EventHandler::handle(&SearchIndexer, &w.ports, &bad)).is_err());
    assert!(w.ports.search_queue.targets().is_empty());
}

#[test]
fn a_market_rebuilds_for_what_prices_depend_on_and_not_for_its_name() {
    use hypha_search::handler::targets;
    let market = hr_market().to_string();
    let marks = |data| targets(&event("market.changed", data));
    assert_eq!(
        marks(serde_json::json!({"market": market, "fields": ["name"]})),
        Ok(vec![])
    );
    assert_eq!(
        marks(serde_json::json!({"market": market, "fields": ["name", "countries"]})),
        Ok(vec![Target::All])
    );
    // Written before `fields` existed: rebuilt, to be safe.
    assert_eq!(
        marks(serde_json::json!({"market": market})),
        Ok(vec![Target::All])
    );
}

#[test]
fn a_price_list_rebuilds_for_what_prices_depend_on_and_not_for_its_name() {
    use hypha_search::handler::targets;
    let marks = |data| targets(&event("price_list.changed", data));
    assert_eq!(
        marks(serde_json::json!({"price_list": "pl_x", "fields": ["name", "description"]})),
        Ok(vec![])
    );
    assert_eq!(
        marks(serde_json::json!({"price_list": "pl_x", "fields": ["name", "ends_at"]})),
        Ok(vec![Target::All])
    );
    // Written before `fields` existed (a creation or a new adjustment): rebuilt.
    assert_eq!(
        marks(serde_json::json!({"price_list": "pl_x"})),
        Ok(vec![Target::All])
    );
}

#[test]
fn a_price_lists_window_rebuilds_where_a_catalog_holds_it() {
    use hypha_search::handler::targets;
    for kind in ["price_list.started", "price_list.ended"] {
        assert_eq!(
            targets(&event(
                kind,
                serde_json::json!({"price_list": "pl_x", "catalogs": ["cat_y"], "starts_at": 1})
            )),
            Ok(vec![Target::All]),
            "{kind}"
        );
        assert_eq!(
            targets(&event(
                kind,
                serde_json::json!({"price_list": "pl_x", "catalogs": [], "ends_at": 1})
            )),
            Ok(vec![]),
            "{kind}"
        );
    }
}

#[test]
fn a_category_rebuilds_its_subtree_when_it_moves_or_hides() {
    use hypha_search::handler::targets;
    let category = hypha_core::CategoryId::from_uuid(Uuid::from_u128(77));
    let marks = |fields: serde_json::Value| {
        targets(&event(
            "category.changed",
            serde_json::json!({"category": category, "created": false, "fields": fields}),
        ))
    };
    assert_eq!(
        marks(serde_json::json!(["visible"])),
        Ok(vec![Target::Category(category)])
    );
    assert_eq!(
        marks(serde_json::json!(["parent", "position"])),
        Ok(vec![Target::Category(category)])
    );
    assert_eq!(marks(serde_json::json!(["name", "position"])), Ok(vec![]));
}

#[test]
fn a_product_only_in_a_hidden_subcategory_isnt_listed_under_its_parent() {
    use hypha_categories::{CategoryRepo, NewCategory};
    let w = World::new();
    let cid = |n| hypha_core::CategoryId::from_uuid(Uuid::from_u128(n));
    let category = |n, parent: Option<u128>, visible| {
        let mut c = hypha_categories::Category::new(NewCategory {
            id: cid(n),
            name: format!("C{n}"),
            handle: hypha_core::Handle::new(&format!("c{n}")).unwrap(),
            description: None,
            parent: parent.map(cid),
            position: 0,
            visible,
        })
        .unwrap();
        block_on(w.ports.categories.insert(&mut c)).unwrap();
    };
    category(900, None, true); // Clothing
    category(901, Some(900), false); // Secret sale, hidden
    block_on(w.ports.categories.add_products(cid(901), &[shirt()])).unwrap();
    block_on(w.ports.categories.add_products(cid(900), &[mug()])).unwrap();
    w.settle();
    for doc in w.documents(hr_market()) {
        let expected = if doc.product == mug() {
            vec![cid(900)]
        } else {
            vec![]
        };
        assert_eq!(doc.categories, expected, "{}", doc.title);
    }
}

// ---------- the search app ----------

fn set_search_app(w: &World, app: Option<&str>) -> Result<Option<String>, IndexError> {
    block_on(update_search_settings(
        &Ctx::system(),
        &w.ports,
        &w.ports,
        UpdateSearchSettings {
            search_app: Some(app.map(str::to_string)),
            ..UpdateSearchSettings::default()
        },
    ))
    .map(|s| s.search_app().map(|a| a.to_string()))
}

#[test]
fn a_search_app_must_answer_search_queries() {
    use hypha_apps::{App, AppRepo, ExtensionEndpoint, ExtensionEndpointRepo, NewApp};
    let w = World::new();
    let key = hypha_core::AppKey::new("app_find").unwrap();
    assert_eq!(
        set_search_app(&w, Some("app_find")),
        Err(IndexError::Search(SearchError::SearchAppUnavailable(
            key.clone()
        )))
    );
    assert!(matches!(
        set_search_app(&w, Some("find")),
        Err(IndexError::Search(SearchError::SearchApp(_)))
    ));
    let mut app = App::new(NewApp {
        key: key.clone(),
        id: hypha_core::AppId::from_uuid(uuid::Uuid::from_u128(0xf1d)),
        name: "Find".to_string(),
        secret: hypha_core::Sealed::from_bytes(vec![1]),
        webhook: None,
        subscriptions: vec![],
    })
    .unwrap();
    block_on(w.ports.apps.insert(&mut app)).unwrap();
    let endpoint = ExtensionEndpoint {
        point: hypha_search_index::SEARCH_POINT.to_string(),
        version: 1,
        url: hypha_core::CallbackUrl::new("https://find.example/q").unwrap(),
    };
    block_on(
        w.ports
            .extension_endpoints
            .replace_for_app(&key, &[endpoint]),
    )
    .unwrap();
    assert_eq!(
        set_search_app(&w, Some("app_find")),
        Ok(Some("app_find".to_string()))
    );
    let stored = block_on(get_search_settings(&Ctx::system(), &w.ports)).unwrap();
    assert_eq!(stored.search_app(), Some(&key));
    // Back to the built-in engine.
    assert_eq!(set_search_app(&w, None), Ok(None));
}

// ---------- custom fields as facets ----------

mod fields {
    use std::collections::BTreeMap;

    use hypha_custom_fields::{
        Access, AdminAccess, CallerKind, Capabilities, DefinitionSpec, FieldCaller, FieldType,
        Owner, OwnerType, SetCustomFields, StorefrontAccess, create_definition, set_custom_fields,
    };
    use hypha_testkit::SequentialIds;
    use serde_json::{Value, json};

    use super::*;

    const SHOWN: Access = Access {
        admin: AdminAccess::ReadWrite,
        storefront: StorefrontAccess::Read,
    };
    const FILTERABLE: Capabilities = Capabilities {
        filterable: true,
        unique: false,
    };

    fn spec(
        owner_type: OwnerType,
        key: &str,
        field_type: &str,
        validations: Value,
        access: Access,
        capabilities: Capabilities,
    ) -> DefinitionSpec {
        DefinitionSpec {
            owner_type,
            key: key.to_string(),
            field_type: FieldType::parse(field_type).unwrap(),
            name: key.to_string(),
            description: None,
            validations: serde_json::from_value(validations).unwrap(),
            access,
            private: false,
            personal: false,
            capabilities,
        }
    }

    fn set(w: &World, owner: Owner, values: Value) {
        let ctx = Ctx::system();
        let kind = CallerKind::Merchant;
        let values: BTreeMap<String, Value> = serde_json::from_value(values).unwrap();
        block_on(set_custom_fields(
            &FieldCaller::new(&ctx, &kind),
            &w.ports,
            &owner,
            SetCustomFields {
                values,
                ..SetCustomFields::default()
            },
        ))
        .unwrap();
    }

    /// The shirt's fields: a material on the product and on Red/M (silk wins there), a
    /// choice, a list, a boolean, an integer; and fields that can't be facets (staff
    /// only, not filterable, a decimal).
    fn world() -> World {
        let w = World::new();
        let ids = SequentialIds::default();
        let d = |owner, key, ty, validations, access, caps| {
            let spec = spec(owner, key, ty, validations, access, caps);
            block_on(create_definition(&Ctx::system(), &w.ports, &ids, spec)).unwrap();
        };
        d(
            OwnerType::Product,
            "material",
            "text",
            json!({}),
            SHOWN,
            FILTERABLE,
        );
        d(
            OwnerType::Variant,
            "material",
            "text",
            json!({}),
            SHOWN,
            FILTERABLE,
        );
        let fits = json!({ "choices": [
            { "key": "slim", "label": "Slim fit" },
            { "key": "loose", "label": "Loose fit" },
        ] });
        d(OwnerType::Product, "fit", "choice", fits, SHOWN, FILTERABLE);
        d(
            OwnerType::Product,
            "care",
            "list.text",
            json!({}),
            SHOWN,
            FILTERABLE,
        );
        d(
            OwnerType::Product,
            "organic",
            "boolean",
            json!({}),
            SHOWN,
            FILTERABLE,
        );
        d(
            OwnerType::Product,
            "gsm",
            "integer",
            json!({}),
            SHOWN,
            FILTERABLE,
        );
        d(
            OwnerType::Product,
            "cost",
            "text",
            json!({}),
            Access::default(),
            FILTERABLE,
        );
        d(
            OwnerType::Product,
            "note",
            "text",
            json!({}),
            SHOWN,
            Capabilities::default(),
        );
        d(
            OwnerType::Product,
            "weight",
            "decimal",
            json!({}),
            SHOWN,
            FILTERABLE,
        );
        set(
            &w,
            Owner::product(shirt()),
            json!({
                "custom.material": "linen",
                "custom.fit": "slim",
                "custom.care": ["hand wash", "cold", "hand wash"],
                "custom.organic": true,
                "custom.gsm": 180,
                "custom.cost": "12",
                "custom.note": "secret",
                "custom.weight": "0.25",
            }),
        );
        set(
            &w,
            Owner::variant(shirt(), red_m()),
            json!({ "custom.material": "silk" }),
        );
        w.settle();
        w
    }

    fn fields_of(w: &World, variant: VariantId) -> Vec<(String, String)> {
        w.documents(hr_market())
            .into_iter()
            .find(|d| d.variant == variant)
            .unwrap()
            .fields
    }

    fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn documents_hold_storefront_filterable_values_the_variants_winning() {
        let w = world();
        let product = [
            ("custom.care", "hand wash"),
            ("custom.care", "cold"),
            ("custom.fit", "slim"),
            ("custom.gsm", "180"),
            ("custom.material", "linen"),
            ("custom.organic", "true"),
        ];
        assert_eq!(fields_of(&w, red_s()), pairs(&product));
        let mut silk = product;
        silk[4] = ("custom.material", "silk");
        assert_eq!(fields_of(&w, red_m()), pairs(&silk));
        assert_eq!(fields_of(&w, mug_v()), vec![]);
    }

    #[test]
    fn a_value_change_rebuilds_its_product() {
        let w = world();
        set(
            &w,
            Owner::product(shirt()),
            json!({ "custom.material": "hemp" }),
        );
        w.settle();
        assert!(fields_of(&w, red_s()).contains(&("custom.material".into(), "hemp".into())));
        // Red/M keeps its own.
        assert!(fields_of(&w, red_m()).contains(&("custom.material".into(), "silk".into())));
    }

    fn set_facets(w: &World, keys: &[&str]) -> Result<(), CommandError> {
        block_on(hypha_search::update_search_settings(
            &Ctx::system(),
            &w.ports,
            &w.ports,
            UpdateSearchSettings {
                facets: Some(keys.iter().map(|k| (k.to_string(), None)).collect()),
                ..UpdateSearchSettings::default()
            },
        ))
        .map(|_| ())
    }

    #[test]
    fn only_filterable_storefront_fields_of_discrete_types_can_be_facets() {
        let w = world();
        for key in [
            "custom.cost",
            "custom.note",
            "custom.weight",
            "custom.nothing",
        ] {
            assert_eq!(
                set_facets(&w, &["vendor", &format!("field.{key}")]),
                Err(CommandError::Search(SearchError::FacetFieldNotAllowed(
                    key.to_string()
                ))),
                "{key}"
            );
        }
        assert_eq!(
            set_facets(
                &w,
                &[
                    "field.custom.material",
                    "field.custom.fit",
                    "field.custom.care",
                    "field.custom.organic",
                    "field.custom.gsm",
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn field_facets_count_filter_and_label_choices() {
        let w = world();
        set_facets(
            &w,
            &[
                "field.custom.material",
                "field.custom.fit",
                "field.custom.organic",
            ],
        )
        .unwrap();
        let material = FacetKey::Field("custom.material".to_string());
        let fit = FacetKey::Field("custom.fit".to_string());
        let found = w
            .search(SearchInput {
                group: Group::Variant,
                filters: vec!["field.custom.material:silk".to_string()],
                ..input("HR")
            })
            .unwrap();
        assert_eq!(found.results.total, 1);
        assert_eq!(found.results.hits[0].document.variant, red_m());
        let counts: Vec<(String, u64)> = found.results.facets[0]
            .values
            .iter()
            .map(|v| (v.value.clone(), v.count))
            .collect();
        // The filter's own facet ignores it.
        assert_eq!(
            counts,
            vec![("linen".to_string(), 2), ("silk".to_string(), 1)]
        );
        assert_eq!(found.results.facets[1].key, fit);
        assert_eq!(
            found.value_labels.get(&fit).and_then(|l| l.get("slim")),
            Some(&"Slim fit".to_string())
        );
        assert!(!found.value_labels.contains_key(&material));
        let organic = FacetKey::Field("custom.organic".to_string());
        assert_eq!(
            found.value_labels.get(&organic).and_then(|l| l.get("true")),
            Some(&"Yes".to_string())
        );
    }

    #[test]
    fn field_events_mark_their_product_or_everything() {
        use hypha_search::handler::targets;
        let updated = json!({ "product": shirt(), "variant": null, "changes": [] });
        assert_eq!(
            targets(&event("product.custom_fields_updated", updated)),
            Ok(vec![Target::Product(shirt())])
        );
        let definition = |owner: &str, fields: Value| {
            json!({ "definition": "cfd_x", "owner_type": owner, "namespace": "custom",
                    "key": "material", "fields": fields })
        };
        let updated = "custom_field_definition.updated";
        for (owner, fields, expected) in [
            ("product", json!(["access"]), vec![Target::All]),
            (
                "variant",
                json!(["capabilities", "name"]),
                vec![Target::All],
            ),
            ("product", json!(["name", "validations"]), vec![]),
            ("customer", json!(["access"]), vec![]),
        ] {
            assert_eq!(
                targets(&event(updated, definition(owner, fields.clone()))),
                Ok(expected),
                "{owner} {fields}"
            );
        }
        let deleted = "custom_field_definition.deleted";
        let gone = |owner: &str| {
            json!({ "definition": "cfd_x", "owner_type": owner, "namespace": "custom",
                    "key": "material" })
        };
        assert_eq!(
            targets(&event(deleted, gone("product"))),
            Ok(vec![Target::All])
        );
        assert_eq!(targets(&event(deleted, gone("order"))), Ok(vec![]));
        assert!(EventHandler::<InMemoryPorts>::wants(
            &SearchIndexer,
            deleted
        ));
        assert!(!EventHandler::<InMemoryPorts>::wants(
            &SearchIndexer,
            "custom_field_definition.created"
        ));
    }
}
