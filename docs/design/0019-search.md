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
| SE11 | **Two crates: a leaf for the model and ports, a workflow for building and syncing.** `hypha-search-index` (core only): documents, queries, `reference::search`, the tokenizer, the settings aggregate, the `SearchIndex`, `SearchQueue` and settings ports, and the settings, reindex and status commands. `hypha-search` (workflow): `build_documents`, `SearchIndexer`, `sync`, and `search` (it resolves the market). Storage adapters implement only core and leaf ports, as before; one crate would have had `hypha-store-postgres` depend on a workflow. *Decided while building (2026-10-16)* |
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
`position`. A hit says how many of its group's documents matched (`matched`).

## 4. The port (SE2)

```rust
// hypha-search/src/repo.rs (as built)
pub trait SearchIndex {
    /// Makes `documents` the documents of `product` in `market`: adds, replaces, and
    /// removes the ones not in `documents` (all of them when it's empty).
    async fn replace(&self, market: MarketId, product: ProductId, documents: &[SearchDocument]) -> Result<(), IndexError>;
    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError>;
    /// Option names (lowercased) in the market, most documents first: default facets.
    async fn option_names(&self, market: MarketId) -> Result<Vec<String>, IndexError>;
    /// Documents over every market: none means a new engine (the host rebuilds).
    async fn count(&self) -> Result<u64, IndexError>;
}
```

The engine isn't a request port (`HasXxx`): a host picks one for its life and passes
it to the commands and the sync job, like the `WebhookSender`. The Postgres engine can
be the request's ports themselves (`PgStore` implements it).

`SearchQuery { market, terms, filters, price: PriceBounds, available, sort, group, facets, offset, limit }`
and `SearchResults { hits: Vec<Hit { document, matched }>, total, facets: Vec<FacetCounts>, price: Option<(Money, Money)> }`.

**What a search means is a plain function, `hypha_search::reference::search`** over a
slice of documents. The in-memory engine runs it; the `search_index` contract loads a
fixture into an engine and requires every query of a battery (terms, filters, price,
availability, every group and sort, pages, facets) to answer exactly as the reference
does. So engines agree on everything, ranking included:

- **Terms** (`terms.rs`, one tokenizer for documents and queries): runs of letters and
  digits, lowercased ("LS-RED" is `ls`, `red`). A document matches when every query
  term is a prefix of one of its terms.
- **Score** (relevance): per query term, 10 if it's in the title or variant title, 4 in
  vendor, type, tags, SKU or option values, 1 in the description; summed.
- **Filters** (`FacetKey`: `option.<name>` with the name lowercased, `vendor`,
  `product_type`, `tag`; exact values): values of a key OR, keys AND (Shopify's rule).
- **Groups:** a group's representative is its first document by the sort (cheapest
  for `price_asc`, dearest for `price_desc`, best score for relevance), then by
  `position`; `matched` counts the group's matching documents.
- **Order** of groups: the sort's key, then newest product (created, then product ID,
  descending), then listing key and position. Titles compare lowercased by code point
  (`COLLATE "C"` in Postgres), so the order doesn't depend on a database's locale.
- **Facets** count **groups** (what the shopper sees), each ignoring its own filter, so
  picking "Red" still shows "Blue" with its count (disjunctive facets). The price range
  ignores the price bounds.
- `sort`: `relevance` (without terms: `newest`), `newest`, `price_asc`, `price_desc`,
  `title`. Pages are offsets, at most 100 hits and 1 000 deep (`MAX_LIMIT`,
  `MAX_DEPTH`); the API wraps them in an opaque `cursor` (0012).
- `IndexError(String)`: the engine failed (clients see `search_unavailable`); bad
  queries are `SearchError`s, caught before the engine.

## 5. Sync (SE5)

**The queue** (`SearchQueue`): one row per target (`Target::Product`,
`Target::Variant`, `Target::All`) with a `generation`. Marking a queued target bumps its
generation instead of adding a row, which is the coalescing: 500 checkouts of one shirt
are one rebuild. Marks are leased like deliveries (oldest marked first); finishing
removes a mark only at the generation leased, so one marked again meanwhile stays, with
its lease freed. The queue needs no clock to mark, so the handler needs none.

**The handler, `SearchIndexer`** (`search_index`, in `hypha-search`, an
`EventHandler` the host lists; section 5.1), marks:

| Event | Marks |
| --- | --- |
| `product.created`, `.updated`, `.variant_added`, `.variant_updated`, `.variant_removed` | the product |
| `catalog.products_added` / `_removed` | those products |
| `price_list.prices_set` / `_removed`, `inventory_item.changed` / `.deleted` | the variants (their products are looked up when the job runs) |
| `market.changed`, `catalog.changed`, `price_list.changed`, `location.changed` (new, section 8) | everything (rare and wide) |
| `search_settings.changed` naming `split_by` | everything; facets alone change no document |

A payload that isn't what its type promises fails the delivery (retried, then shown in
the admin) rather than being skipped.

**The job, `hypha_search::sync`**, leases a batch of marks, then:
- **all:** marks every product, a page at a time (products aren't deleted and markets
  aren't removed, so every product rebuilt covers every market);
- **variant:** its product (a removed variant's product has its own event);
- **products:** loads them once; per market, loads prices and stock once for all of
  them (`load_pricing`, `StockContext::load`) and `replace`s each product's documents
  (none when it's inactive, unlisted or gone);

and finishes the marks only if everything worked; otherwise their lease runs out and a
later run tries again. It answers `Progress` like the dispatcher's jobs.

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
- `GET /storefront/search?country=DE&q=shirt&filter=option.color:Red&filter=option.color:Blue&filter=vendor:Acme&price_min=1000&price_max=5000&available=true&sort=price_asc&group=listing&limit=24&cursor=…`
  - `country` is required: the market is the one serving it (`422 country_not_served`
    otherwise).
  - `filter` repeats; `key:value`, key a `FacetKey`: `option.<name>` (any case),
    `vendor`, `product_type`, `tag`. Values within a key OR, keys AND. Options carry a
    prefix so an option called "Vendor" or "Tag" can't be mistaken for a field.
  - Response: `{ items: [{ product, variant, listing, handle, title, variant_title,
    price, product_price: {min, max}, listing_price, available, image: {url, alt}?,
    options, matched }], total, facets: [{ key, label, values: [{ value,
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
7. Meilisearch, as a spore (deferred, see section 13): an engine implementing
   `SearchIndex` (HTTP through a port, like `hypha-store-s3`), the contract against a
   Meilisearch in `docker-compose.yml`, chosen in `hypha.config.toml`.

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

**Q3. Facet counts on engines that count documents.** *Open until Meilisearch (step
7).* The contract holds engines to group counts, which Postgres and the reference give.
If Meilisearch's facet distribution counts documents even with `distinct`, either the
adapter counts groups itself (B: slower) or the contract gains a documented allowance
for that engine (A).

**Q4. `country` vs `market` on the storefront.** *Proceeding with country*, as carts
do: a storefront knows where the shopper is, not our market IDs.

## 12. Later

- Storefronts querying the engine directly with scoped keys (InstantSearch).
- Collections (manual and rule-based) as a filter and a listing source; a taxonomy.
- Synonyms, stop words, pinning and boosting per query; search analytics.
- Documents per language when translations exist; UCP catalog search over this index.

## 13. As built (2026-10-16)

Built on `feat/search` in section 10's order, with three refactors first (handlers as a
trait, SE10; shared pricing and stock loaders; `Progress` in core) and the crate split
(SE11). Step 6 built the admin's Settings → Search page (listings split by options,
the storefront's filters in order with labels, the index's status with a rebuild
button that follows its progress). *Decided (2026-10-16):* step 7, Meilisearch, will be
a spore, built when a store needs it: Postgres is the engine out of the box, and Q3 is
decided with the spore.

- **Products** gained `description` (sanitised HTML, `ammonia`), `vendor`,
  `product_type` and `tags` (migration 0033). Below a 256 KiB `server.max_body`, the
  longest description may not fit; the limit's minimum stays 1 KiB (operators' values
  stay valid). The admin list filters by `q` (title, handle, SKU; `strpos`
  on `lower()`, so `%` and `_` are plain text).
- **Locations** record `location.changed` naming the fields that changed; search
  rebuilds everything only when `markets` changed.
- **The engine seam** is `Storage::search(&ports)`: the Postgres storage lends its
  ports; Meilisearch will be held by the storage and chosen by configuration.
- **Jobs:** `[jobs.search_sync]` (`enabled`, `interval` 2 s, `batch` 50, `lease` 2 min,
  at least 10 s). An empty index at startup queues a full rebuild.
- **Routes:** `GET /storefront/search` (`country`, `q`, repeatable `filter`,
  `price_min`/`price_max`, `available`, `sort`, `group`, `limit`, `cursor`; the cursor
  carries the offset and only continues the same query), `GET`/`PATCH
  /admin/search/settings`, `GET /admin/search/status`, `POST /admin/search/reindex`
  (202). Engine failures are `503 search_unavailable` with the cause only in the log.
  List filters now parse with `serde_html_form`, so a repeated key fills a `Vec`.
- **Tests:** the reference by hand (13), the engine contract (330 queries against the
  reference) on memory and Postgres, the queue and settings contracts, command and
  handler tests, route tests, and an e2e section.

### Review and performance (2026-10-16)

`/code-review high` and a load test (documents seeded straight into Postgres, searches
timed through the HTTP API, release build) led to:

- **Descriptions:** the 64 KiB limit applies to the sanitised HTML too (escaping can
  grow it), so an accepted description always loads again; `restore` no longer
  sanitises (it was parsing up to 64 KiB of HTML on every product load, carts included)
  and only checks the length (`Description::stored`).
- **Paging:** a page that would end past 1,000 results is cut short there, and
  `next_cursor` stops at the limit; only starting at 1,000 or later is `422
  search_too_deep`.
- **Sync:** failures are per product (one the engine refuses or that can't load fails
  alone; the rest of its batch is finished); variant marks resolve in one query
  (`ProductRepo::ids_of_variants`); stock items load once per step, not per market; each
  market's batch is one `replace` (`SearchIndex::replace` takes a batch of products). A
  runner whose lease ran out and finds its mark gone (`SearchQueue::finish` returns
  those) marks the target again, so its possibly older documents are rebuilt.
- **Terms** longer than 100 characters are dropped (`MAX_TERM_CHARS`); engines cap
  lexemes too, and this keeps them in agreement. Postgres builds `tsv` with
  `array_to_tsvector` of the terms and queries with `'term':*`, so its parser plays no
  part and the index alone decides a match (migration 0035; it empties the index, which
  the host rebuilds at start).
- **The Postgres search** is one statement: narrow matched rows, the group sort without
  the facets, product-level facets (vendor, type, tags: the same on every variant)
  counted once per group from its representative, `DISTINCT` skipped for
  `group=variant`, 64 MB `work_mem` for the transaction; default option facets come
  from `search_option_counts`, kept as documents are written.
- `server.max_body` keeps its 1 KiB minimum (operators' values stay valid); below 256
  KiB a full description may not fit. `SyncError` implements `ErrorCode`.

Measured (one market; products of 10 variants; full listing = no `q`, no filters, all
default facets):

| Documents | Full listing | Filtered or text searches |
| --- | --- | --- |
| 20,000 | ~95 ms | 8–100 ms |
| 50,000 | ~190 ms | 7–145 ms |
| 200,000 | ~0.75 s (was 5 s) | 5–180 ms; full-market filters up to ~0.5 s |

A full rebuild of 1,000 products (`POST /admin/search/reindex`) takes ~1.5 s: a step
that expands a full rebuild says there's more, so the job doesn't wait its interval.

**Guidance:** the built-in engine serves a market of up to about 50,000 variant
documents well. Past that, counting distinct listings over every matching row is the
floor in SQL; that's what step 7's Meilisearch engine is for.

