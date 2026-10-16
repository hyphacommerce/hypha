# 0019: Search and storefront product listing

- **Status:** Accepted (2026-10-16). Danilo set the frame: core content fields on
  products; documents per market with prices resolved; a `SearchIndex` port with a
  built-in Postgres engine and Meilisearch as the first spore; the admin's product
  search reads the database. On the document shape he asked for flexibility (some
  clients want products, others variants), which became SE3 and SE4. He then said to
  build it; section 11's questions are answered with their recommendations, marked
  *Proceeding with*, until he says otherwise. On Q2 he asked for the cleanest
  architecture rather than either option, which became SE10 (*Decided*, 2026-10-16).
- **Scope:** product content fields (description, vendor, product type, tags), the
  admin product list's text filter, search documents and how they stay in sync, the
  `SearchIndex` port and its engines, storefront search and listing with facets and
  prices per market, the merchant's search settings, routes and admin screens.
- **Not in scope:** collections and a taxonomy, custom fields in documents (0002 §3;
  section 9 reserves the place), synonyms and merchandising rules (pinning, boosting),
  search analytics, autocomplete as its own endpoint, letting storefronts query an
  engine directly (scoped engine keys), per-language documents, UCP's catalog search.
- **Related:** 0010 (S5: storefront prices per market come with product listing),
  0012 (lists and cursors), 0013/0014 (media; the featured image), 0016 (events, the
  dispatcher, handlers, jobs; "search takes the latest per variant"), 0017 (settings),
  0002 (custom fields, spores).

## Decisions

| | Decision |
| --- | --- |
| SE1 | **Products gain content fields**: `description` (sanitised HTML), `vendor`, `product_type` and `tags` (up to 250, unique ignoring case), as Shopify has. They're what shoppers search and what facets are made of. *Hard to reverse (description as HTML)* |
| SE2 | **A `SearchIndex` port** in a new workflow crate **`hypha-search`**, which also builds documents, keeps the merchant's settings and runs the sync. Engines are adapters: **Postgres built in** (`hypha-store-postgres`), **Meilisearch** the first external one; Algolia and Typesense later through the same port |
| SE3 | **One document per variant per market.** It carries its product's fields too. Only active products in the market's catalog, priced in the market's currency (`resolve_price`), with availability from the locations serving the market. Filters are exact: "Red" and "XL" match only a variant that is both. *Hard to reverse* |
| SE4 | **Grouping is chosen per query, not per index.** Each document carries `product`, `variant` and `listing` keys; a query's `group` is `listing` (default), `product` or `variant`, and every engine collapses on it (`distinct`, `group_by`, `DISTINCT ON`). `listing` is the product, or the product split by options the merchant names ("show each Color separately"). *Hard to reverse (the `group` parameter)* |
| SE5 | **Kept in sync through the outbox, coalesced.** A dispatch handler, `search_index`, turns events into dirty marks (a product, a market, or everything) in a `search_queue`; a job drains the queue, rebuilds each dirty product's documents per market and replaces them in the engine. A busy product is rebuilt once however many events it had |
| SE6 | **The storefront asks Hypha, not the engine.** `GET /storefront/search` takes a country (as carts do) to pick the market, `q`, filters, sort, group and a page; it returns hits with prices and facet counts. With no `q` it's the product listing 0010 S5 waited for. *Hard to reverse* |
| SE7 | **Default facets** are each option name (`Color`, `Size`), `vendor`, `product_type`, `tag`, price and availability. The merchant picks which are shown and in what order, and which options split listings (`SearchSettings`, one per store) |
| SE8 | **The admin searches the database**: `GET /admin/products?q=` matches title, handle and SKU (case-insensitive substring), so staff see drafts and their own edits at once. The index is for shoppers |
| SE10 | **Event handlers are a trait, and the host composes them.** `hypha_core::EventHandler<P>` (name, the event types it wants, `handle`); the dispatcher runs whatever tuple of handlers the host passes, and each handler bounds `P` on just the repositories it uses. Search's handler lives in `hypha-search`; `hypha-dispatch` doesn't know it. Spores add handlers the same way (0002 §6). *Decided* |
| SE9 | **The Postgres engine needs no extension**: full-text search with the `simple` configuration and prefix matching. Typo tolerance and ranking finesse are what Meilisearch is for |

## 1. What others do

**Shopify** ([Storefront `search`](https://shopify.dev/docs/api/storefront/latest/queries/search), [`ProductFilter`](https://shopify.dev/docs/api/storefront/latest/input-objects/ProductFilter), [Search & Discovery filters](https://help.shopify.com/en/manual/online-store/search-and-discovery/filters), [combined listings](https://help.shopify.com/en/manual/products/combined-listings-app))
- Products have `descriptionHtml`, `vendor`, `productType` and `tags` (250 tags of 255
  characters); search matches them, plus SKUs and option values.
- Storefront search and collection listings take filters: availability, price range,
  product type, vendor, tags, variant options, metafields. Results carry the filters
  with counts.
- The merchant picks which filters show, and in what order, in the Search & Discovery
  app. Products are the unit; showing each colour as its own card needs combined
  listings (several products presented as one), a workaround for the missing option.
- Prices follow the shopper's market (`@inContext(country:)`).

**Medusa v2** ([search with Meilisearch / Algolia](https://docs.medusajs.com/resources/integrations/guides/meilisearch))
- No search in core: a module or plugin indexes product documents on product events,
  and the storefront queries the engine directly with a search-only key.
- One document per product; prices per region aren't in the index by default.

**Vendure** ([`DefaultSearchPlugin`](https://docs.vendure.io/reference/typescript-api/default-search-plugin/), [`ElasticsearchPlugin`](https://docs.vendure.io/reference/core-plugins/elasticsearch-plugin/))
- A built-in database search (SQL full-text) and an Elasticsearch plugin behind the same
  `search` query. The index is **one row per variant per channel and language**, with
  prices in the channel's currency.
- `groupByProduct: true` on the query collapses variants into products, with price
  ranges. Facets are first-class entities assigned to products and variants.
- The index is updated by a job queue on entity events, with a full reindex mutation.

**Saleor** ([search](https://docs.saleor.io/developer/searching))
- Products carry a search vector in Postgres, updated by a background task when products
  change. Prices per channel are listed on the product, filterable per channel.

**Meilisearch, Algolia, Typesense** all collapse results on an attribute at query time
(Meilisearch `distinct`, which must be a filterable attribute; Algolia `distinct`;
Typesense `group_by`), so a per-query `group` costs nothing extra with any of them.

**What we take:** the content fields and default filters (Shopify); a built-in engine
and plugin engines behind one search API, variant-level rows per channel with prices,
and grouping on the query (Vendure); the merchant choosing filters (Shopify); a
coalescing queue fed by events with a full reindex (Vendure, Saleor).

**What we don't take, yet:** querying the engine directly from the storefront (Medusa:
it skips our market rules and rate limits; a later option with scoped keys), Vendure's
facet entities (tags and options do the job until a taxonomy), combined listings
(SE4's option split covers the common case without extra products).

## 2. Content fields (SE1)

On `Product` (`hypha-products`):

| Field | Rule |
| --- | --- |
| `description` | `Option<String>`, sanitised HTML up to 64 KiB. The sanitiser (an allow-list: paragraphs, headings, lists, emphasis, links, tables, images; no scripts, styles or event attributes) runs in the constructor, so stored HTML is always safe to render. Empty after sanitising is `None` |
| `vendor` | `Option<String>`, the shared text rule (trimmed, ≤ 255) |
| `product_type` | `Option<String>`, the same |
| `tags` | `Vec<String>`, each by the text rule, ≤ 250, unique ignoring case (the first spelling wins), kept in the order given |

- `UpdateProduct` gains the four as merge-patch fields; `product.updated` lists them by
  name in `fields`, as it does for `title`.
- `ProductSummary` gains `vendor` and `product_type` (the list shows them).
- Migration: four columns on `products` (`tags text[] NOT NULL DEFAULT '{}'`).
- Admin and storefront product responses carry them; the admin's product page gets a
  description editor and the three fields.

The search text of a description is its HTML with tags removed (`Product::description_text`).

## 3. Documents (SE3, SE4)

```rust
/// One variant of one active product, as a shopper in `market` sees it.
pub struct SearchDocument {
    pub market: MarketId,
    pub product: ProductId,
    pub variant: VariantId,
    /// What `group=listing` collapses on: the product ID, or with option splits the
    /// product ID and the split options' values ("prod_…:Color=Red").
    pub listing: String,
    pub handle: String,
    pub title: String,
    pub variant_title: Option<String>,   // "M / Blue"
    pub description: String,             // text, tags removed
    pub vendor: Option<String>,
    pub product_type: Option<String>,
    pub tags: Vec<String>,
    pub sku: Option<String>,
    /// Option name → value label, in option order ({"Color": "Red", "Size": "M"}).
    pub options: Vec<(String, String)>,
    pub price: Money,                    // in the market's currency
    /// Over the product's and the listing's variants in this market, for cards.
    pub product_price: PriceRange,
    pub listing_price: PriceRange,
    pub available: bool,
    /// The variant's image, else the product's featured one.
    pub image: Option<AssetId>,
    pub created_at: Timestamp,           // the product's, for "newest"
    /// Its position in the product (variant order), to pick a listing's representative.
    pub position: u32,
}
```

- **Built by a pure function**, `build_documents(product, market context, stock,
  settings) -> Vec<SearchDocument>`, from the same contexts the cart prices with
  (`PricingContext`, `StockContext`), so search and cart can't disagree on a price.
- **Left out:** a product that isn't active or isn't in the market's catalog; a variant
  whose price doesn't resolve (another currency). A market without a catalog has no
  documents.
- **Available** means some serving location has stock, the item is untracked, or it
  sells past zero (`continue_selling`), as the cart decides.
- **Option names** are matched ignoring case, so "Color" and "color" are one facet; the
  facet is labelled with the first spelling seen.
- **Images are asset IDs**: the host turns them into URLs when it answers (`MediaUrls`),
  so a CDN change needs no reindex.
- **Size:** variants × markets documents. 10 000 products with 10 variants in 3 markets
  is 300 000 rows, well within Postgres and Meilisearch.

**Group keys and the representative:** with `group=product` or `listing`, the hit is
the best-ranked document of the group; ties (a listing page with no `q`) go to the lowest
`position`. A hit says how many of its group's documents matched (`matched_variants`)
where the engine can tell.

## 4. The port (SE2)

```rust
#[allow(async_fn_in_trait)]
pub trait SearchIndex {
    /// Makes `docs` the documents of `product` in `market`: inserts, replaces, and
    /// removes the ones not in `docs`. Empty removes the product from the market.
    async fn replace(&self, market: MarketId, product: ProductId, docs: &[SearchDocument]) -> Result<(), SearchError>;
    /// Removes every document of `market` (deleted, or rebuilt from scratch).
    async fn clear_market(&self, market: MarketId) -> Result<(), SearchError>;
    /// The products with documents in `market`, a page at a time (for full rebuilds,
    /// to remove what's no longer there).
    async fn products(&self, market: MarketId, after: Option<ProductId>, limit: u32) -> Result<Vec<ProductId>, SearchError>;
    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, SearchError>;
}
```

`SearchQuery { market, q, filters: Vec<Filter>, price: Option<(Money, Money)>, available: Option<bool>, sort, group, facets: Vec<FacetKey>, offset, limit }`
and `SearchResults { hits: Vec<SearchDocument + matched_variants>, total, facets: Vec<FacetCounts> }`.

- `Filter { key: FacetKey, values: Vec<String> }`: values within a key are OR, keys are
  AND (Shopify's rule). `FacetKey` is `Option(name)`, `Vendor`, `ProductType`, `Tag`.
- Facet counts count **groups** (what the shopper sees), and each facet's counts ignore
  that facet's own filter, so picking "Red" still shows "Blue" with its count
  (disjunctive facets). An engine that can only count documents says so
  (`SearchIndex::COUNTS_GROUPS`); Postgres counts groups.
- `sort`: `relevance` (default with `q`), `newest` (default without), `price_asc`,
  `price_desc`, `title`.
- Pages are offsets (engines page that way) capped at 1 000 results deep; the API wraps
  them in the usual opaque `cursor` (0012), so a page shape change stays internal.
- `SearchError`: `Engine(String)` (unreachable, refused) and `InvalidQuery`; no
  engine's types leak.
- The testkit gets `InMemorySearchIndex` and a `search_index` contract scenario set that
  both engines run.

## 5. Sync (SE5)

```sql
CREATE TABLE search_queue (
    target     text        PRIMARY KEY,  -- 'product:<uuid>', 'market:<uuid>', 'all'
    queued_at  timestamptz NOT NULL,
    leased_until timestamptz
);
```

**The handler, `SearchIndexer`** (`search_index`, in `hypha-search`, an
`EventHandler` the host lists; section 5.1), marks:

| Event | Marks |
| --- | --- |
| `product.created`, `.updated`, `.variant_added`, `.variant_updated`, `.variant_removed`, `.deleted` | the product |
| `catalog.products_added` / `_removed` | those products |
| `catalog.changed`, `market.changed` | the market |
| `price_list.changed` | everything (rare; markets using it aren't in the event) |
| `price_list.prices_set` / `_removed`, `inventory_item.changed` / `.deleted` | the variants' products (looked up when the job runs) |
| `location.changed` (new, section 8) | everything |
| `search_settings.changed` (new) | everything |

A mark that's already queued stays one row (`ON CONFLICT DO NOTHING`), which is the
coalescing: 500 checkouts of one shirt are one rebuild.

**The job, `hypha_search::sync`**, leases a batch of marks (like deliveries), then:
- **product:** loads it once, and per market builds and `replace`s its documents (empty
  where it's no longer listed, or deleted);
- **market:** walks the market's catalog's products in pages, re-marking each as a
  product (so the work stays bounded per run), then removes documents of products
  no longer in it (`SearchIndex::products`);
- **all:** re-marks every market, and clears markets that no longer exist;

then deletes the marks it finished. A mark re-queued while it ran (the product changed
again) is kept: the delete matches `queued_at`. Failures leave the mark for the next run;
`Progress` like the dispatcher's.

### 5.1 Handlers as a trait (SE10)

Until 0019 the dispatcher knew every handler: a constant list of names, a `match` that
ran them, and `DispatchPorts` bounded on every repository any of them used. Each new
handler meant editing the dispatcher and widening its bounds, and a spore couldn't add
one without editing core, the opposite of 0002 §6.

```rust
// hypha-core (handlers.rs): a port shape like `Clock`, no domain knowledge.
pub trait EventHandler<P: ?Sized> {
    fn name(&self) -> &'static str;            // deliveries go to "handler:<name>"
    fn wants(&self, event_type: &str) -> bool;
    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String>;
}
pub trait EventHandlers<P: ?Sized> {           // (), and tuples of 1 to 12 handlers
    fn names(&self) -> Vec<&'static str>;
    fn wanting(&self, event_type: &str) -> Vec<&'static str>;
    async fn run(&self, name: &str, ports: &P, event: &StoredEvent) -> Option<Result<(), String>>;
}
```

- `fan_out`, `handle`, `run` and `deliver_due` take `handlers: &impl EventHandlers<S>`;
  `DispatchPorts` is back to the outbox and apps.
- Each handler states its own needs: `impl<P: HasPriceLists + HasInventoryItems + …>
  EventHandler<P> for VariantCleanup`. A host whose ports lack them doesn't compile with
  that handler in its set.
- **The host is the composition root:** `hypha-host-axum/src/handlers.rs` holds
  `const HANDLERS: (VariantCleanup, SearchIndexer)`, with a test that names are unique.
  A Workers host lists its own.
- **Why a tuple, not `Vec<Box<dyn EventHandler>>`:** a trait with a generic parameter
  and an `async fn` isn't object safe, and a `dyn` future couldn't be proven `Send` by
  the host. Tuples are static dispatch (the pattern axum uses for handler arguments and
  Bevy for systems): the compiler generates the code for the exact set.
- **Names are a contract:** deliveries store `handler:<name>`. A handler removed from
  the set leaves its pending deliveries retrying ("no handler in this build") until
  they fail, so putting it back in time still delivers.
- **Not a cursor over the outbox:** a projection like search could read events after
  a position of its own, but sequence numbers are taken before commit, so a reader can
  see 11 before 10 commits and skip 10 for good (`events_after` is documented as not a
  feed). Claiming through `fan_out` doesn't have that gap.

**Reindex:** `POST /admin/search/reindex` queues `all`. The host queues `all` at start
when the engine reports it's empty (a new engine, or a switch from another).

## 6. Settings (SE7)

```rust
pub struct SearchSettings {          // one per store, versioned, `search_settings.changed`
    /// Option names (ignoring case) whose values split listings, in order.
    split_by: Vec<String>,           // ≤ 3
    /// Facets the storefront shows, in order; empty means the defaults.
    facets: Vec<FacetSetting>,       // { key: FacetKey, label: Option<String> }, ≤ 20
}
```

- Defaults: no split; facets `available`, `price`, `product_type`, `vendor`, every option
  name in the market (most used first), `tag`.
- An option facet names an option ("Color"); a product without it simply has no value.
- Changing `split_by` changes `listing` keys, so it queues `all`; changing facets
  doesn't touch documents.
- Scopes: reading is `products:read`, changing is `products:write` (search settings are
  how products are presented, like merchandising); no new scope.

Developers adjust beyond this through the search spore seam (section 9).

## 7. API and admin

**Storefront** (publishable key, `products:read`):
- `GET /storefront/search?country=DE&q=shirt&filter=Color:Red&filter=Color:Blue&filter=vendor:Acme&price_min=1000&price_max=5000&available=true&sort=price_asc&group=listing&limit=24&cursor=…`
  - `country` is required: the market is the one serving it (`422 country_not_served`
    otherwise).
  - `filter` repeats; `key:value`, where key is an option name or `vendor`, `type`,
    `tag`. Values within a key OR, keys AND.
  - Response: `{ items: [{ product, variant, listing, handle, title, variant_title,
    price, product_price: {min, max}, listing_price, available, image: {url, alt}?,
    options, matched_variants? }], total, facets: [{ key, label, values: [{ value,
    count }] }] | price: {min, max}, next_cursor }`.
- `GET /storefront/products` stays absent: the listing is `search` without `q` (one
  route, one response shape). `GET /storefront/products/{id}` gains the content
  fields.

**Admin:**
- `GET /admin/products?q=…` (SE8), with the existing `status`.
- `GET /admin/search/settings`, `PATCH /admin/search/settings` (merge patch),
  `POST /admin/search/reindex`, `GET /admin/search/status` (queued marks, the engine's
  name and document count).
- Admin screens: a search box on the product list; the product form's new fields; a
  Settings → Search page (split options, facet order, reindex, status).

## 8. Changes to existing crates

- `hypha-products`: content fields (section 2), `description_text`, `q` in
  `ProductRepo::list` (a `ProductFilter { status, q }`); `product.deleted` if deleting
  products exists by then, else documents go when the product leaves its catalogs or
  stops being active.
- `hypha-inventory`: `location.changed` when a location's markets change (availability
  depends on it).
- `hypha-cart`: `load_pricing` / `load_stock` become reusable functions over a product
  set (moved where `hypha-search` can call them without depending on carts: into
  `hypha-pricing` and `hypha-inventory`, or a shared module, decided while building).
- `hypha-core`: `EventHandler` and `EventHandlers` (section 5.1).
- `hypha-dispatch`: runs the host's handlers; `variant_cleanup` becomes the
  `VariantCleanup` handler; `DispatchPorts` shrinks to the outbox and apps.
- `hypha-host-axum`: `handlers.rs` lists the set; the host runs `hypha_search::sync` as
  a job (`jobs.search_sync`, every 2 s, in `hypha.config.toml` like the others).

## 9. Extending it (developers)

- **Engines:** implement `SearchIndex` and run the contract.
- **Documents:** `SearchDocument` has `extra: BTreeMap<String, serde_json::Value>`,
  empty in core. A spore fills it through a `DocumentEnricher` (called after
  `build_documents`), and declares which keys are facets or searchable; custom fields
  (0002 §3) will use the same seam. Not built until the first spore needs it.
- **Grouping:** a spore can replace how `listing` is computed (an enricher writing it).

## 10. Build order

Each step ends green (fmt, clippy, tests, wasm, Postgres contract, e2e).

1. Content fields: products (with sanitising), migration, host DTOs, OpenAPI, SDK,
   admin product form, e2e.
1b. Handlers as a trait (SE10): core's traits, the dispatcher and `VariantCleanup` on
   them, the host's set (a refactor commit of its own).
2. Admin `q`: `ProductFilter`, both repos, contract, route, admin search box.
3. `hypha-search`: documents and `build_documents`, settings, queue and sync, the port,
   `InMemorySearchIndex` and the contract, with the shared pricing/stock loaders.
4. Postgres: the engine (`search_documents`), the queue and settings tables, the
   contract.
5. `location.changed`, `SearchIndexer` in the host's set, the host job, routes,
   OpenAPI, SDK, e2e.
6. Admin: Settings → Search.
7. Meilisearch: `hypha-search-meilisearch` (HTTP through a port, like `hypha-store-s3`),
   the contract against a Meilisearch in `docker-compose.yml`, chosen in
   `hypha.config.toml` (`search.engine`).

## 11. Open decisions

**Q1. The description's format.** *Proceeding with A.*
- **A. Sanitised HTML** (Shopify's `descriptionHtml`): what rich-text editors produce
  and storefronts render; the sanitiser (`ammonia`, pure Rust) runs in the domain, so
  nothing unsafe is ever stored.
- **B. Markdown**: safer by nature, but every storefront needs a renderer and imports
  from other platforms arrive as HTML.
- **C. Plain text** now, rich text later: changing a field's meaning later breaks
  clients.

**Q2. How the search handler reaches the dispatcher.** *Decided: neither A nor B,*
SE10. A (`hypha-dispatch` depends on `hypha-search`) kept the dispatcher's handler list
closed and its port bounds growing with every handler; B (the queue port in
`hypha-core`) put a search-only concept into core and still left the dispatcher naming
the handler. Making handlers a trait the host composes removes the edge and the
special case, and gives spores their `event_handlers()` (0002 §6).

**Q3. Facet counts on engines that count documents.** *Proceeding with A.* A. Accept
the difference and say so (`COUNTS_GROUPS`); B. count groups with a second query per
facet (slow).

**Q4. `country` vs `market` on the storefront.** *Proceeding with country*, as carts
do: a storefront knows where the shopper is, not our market IDs.

## 12. Later

- Storefronts querying the engine directly with scoped keys (InstantSearch).
- Collections (manual and rule-based) as a filter and a listing source; a taxonomy.
- Synonyms, stop words, pinning and boosting per query; search analytics.
- Documents per language when translations exist; UCP catalog search over this index.
