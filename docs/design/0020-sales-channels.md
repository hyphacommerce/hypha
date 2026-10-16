# 0020: Sales channels

- **Status:** Proposed (2026-10-16). Danilo raised sales channels after search (0019):
  per-product publishing as in Shopify, an Online Store channel now and an agentic
  channel later, built before the admin's store setup and the storefront. He agreed to
  the shape in section 2 before the note was written (marked *Decided*); section 10
  lists what's left.
- **Scope:** channels (where products are sold), publishing products to them,
  publishable keys belonging to a channel, storefront reads, carts and checkout honouring
  publication, orders recording their channel, channel-aware search, events, routes and
  admin screens.
- **Not in scope:** the agentic channel's adapter (UCP, via ucphub.ai; only its kind is
  reserved), prices or stock per channel (markets and locations do that), scheduled
  publishing, channel-specific product content, POS, marketplace channels (Amazon,
  Google) and their feeds, multi-store tenancy (0006).
- **Related:** 0008 (API keys, `Scope::STOREFRONT`), 0010 (the storefront surface),
  0007 (orders), 0016 (events), 0019 (search documents), 0006 (tenancy), CLAUDE.md
  (agentic shopping through UCP, not the core API).

## Decisions

| | Decision |
| --- | --- |
| CH1 | **A channel is *where* a product is sold; a market is *to whom* and *at what price*.** They stay independent: a storefront request may sell a product only if it is published to the request's channel **and** in the request market's catalog. Prices, currencies, tax and stock stay per market and location. *Decided* |
| CH2 | **A `Channel` aggregate** (`chan_…`): a name and a kind, `online_store`, `agentic` or `custom` (open-ended, like event types: clients must accept kinds they don't know). The kinds only label channels; the rules are the same for all. *Decided* |
| CH3 | **Publications are a set-based membership**, channel ↔ product, written in batches with events (`channel.products_published` / `_unpublished`), like catalog membership. A product's channels show on its admin response. *Decided* |
| CH4 | **A publishable key belongs to exactly one channel; a channel has any number of keys.** The storefront request's channel comes from its key, never from the request, so clients can't forget or forge it. Secret keys and staff sessions belong to none and see every product, as now. *Decided; hard to reverse* |
| CH5 | **`Ctx` carries the caller's channel** (`Ctx::channel() -> Option<ChannelId>`), set by `Caller::authenticate` from the key, next to the actor and scopes. Commands read it; none takes a channel from raw input. *Hard to reverse* |
| CH6 | **The storefront honours publication everywhere:** reading a product (404 when unpublished, like a draft), search and listing, adding to a cart (`422 product_not_published`), a cart's priced lines (`not_published`, like `not_available`), and checkout (refused while a line isn't). A cart records its channel when created. *Decided* |
| CH7 | **Orders record their channel** (copied from the cart), for reporting, for webhooks (`order.placed` gains it) and so agent-placed orders are told apart. *Hard to reverse* |
| CH8 | **Search documents carry their product's channels**; storefront search filters by the caller's channel. One document per variant per market, as before, not per channel too. *Decided* |
| CH9 | **"Online Store" exists from the start**: the migration creates it, gives it every existing publishable key and publishes every existing product to it, so nothing a storefront sees today disappears. New products are published deliberately (no automatic publishing), like Shopify. *Decided* |
| CH10 | **New scopes `channels:read` and `channels:write`** for managing channels; publishing a product to a channel needs `products:write` (it's how a product is presented, like search settings). |

## 1. What others do

**Shopify** ([sales channels](https://help.shopify.com/en/manual/online-sales-channels), [`Publication`](https://shopify.dev/docs/api/admin-graphql/latest/objects/Publication), [`publishablePublish`](https://shopify.dev/docs/api/admin-graphql/latest/mutations/publishablePublish))
- Sales channels are apps (Online Store, POS, Shop, Google, Facebook, headless
  storefronts); each has a publication, and products (and collections) are published to
  publications one by one or in bulk.
- A Storefront API token belongs to a channel (its app), and the Storefront API only
  returns what's published to it.
- Markets are separate: a product published to the Online Store is still sold only in
  the markets whose catalogs include it.
- Orders record their source (channel).

**Medusa v2** ([sales channels](https://docs.medusajs.com/resources/commerce-modules/sales-channel), [publishable API keys](https://docs.medusajs.com/resources/storefront-development/publishable-api-keys))
- Products belong to sales channels; a publishable API key is scoped to sales channels,
  and the store API only shows their products. Carts and orders record their sales
  channel. Stock locations are linked to sales channels.

**Saleor** ([channels](https://docs.saleor.io/developer/channels/overview)) merges
"where" and "to whom": a channel has its currency, countries, prices and product
availability. **Vendure** channels lean towards multi-tenancy (each with its own
products, prices and sellers).

**What we take:** channels as publications with per-product publishing (Shopify,
Medusa); the storefront key deciding the channel (Shopify's tokens, Medusa's
publishable keys); orders recording their channel (both).

**What we don't take:** Saleor's merge of channels and markets (we already have
markets, and keeping "where" apart from "to whom" lets one storefront serve many
countries); Medusa's links from locations to channels (our locations serve markets);
Medusa's several channels per key (one is simpler to reason about, and a second key is
cheap); Vendure's per-channel tenancy (that's 0006).

## 2. Model

**`Channel` (new leaf crate `hypha-channels`, on core; `chan_…`)**
- `name`: the shared text rule, unique ignoring case (`keys::CHANNEL_NAME`).
- `kind`: `ChannelKind` (`online_store`, `agentic`, `custom`), set when created.
- Versioned, stamped, records `channel.changed` (created, renamed).

**Publications** (`ChannelRepo::publish` / `unpublish` / `published`, set-based, in the
same crate)
- `(channel, product)` pairs. Publishing an already published product, or unpublishing
  one that isn't, changes nothing and records nothing.
- `channels_of(products) -> map product → channels`, one query for a batch (cart pricing
  and search use it).
- Staged in a batch with their events (`StagePublications`, like `StageCatalogProducts`).
- Storage keeps the product → publication reference (`ON DELETE CASCADE` when products
  can be deleted; they can't yet).

**Why a leaf of its own:** products, carts, search and auth all need the channel's ID
or its publications, and none of them should depend on another for it. The ID goes in
core (`define_id!(ChannelId, "chan")`); the aggregate and publications in
`hypha-channels`.

## 3. Who is in which channel (CH4, CH5)

- `ApiKey` gains `channel: Option<ChannelId>`: required for a publishable key, refused
  for a secret one (`ApiKeyError::ChannelRequired` / `ChannelNotAllowed`). Creating a
  publishable key names its channel; the channel must exist (`422 channel_not_found`).
- `Ctx` gains `channel: Option<ChannelId>` and `Ctx::channel()`; `Ctx::new` stays as it
  is for staff and system (no channel), `Ctx::in_channel(actor, scopes, channel)` for a
  storefront caller. `Caller::authenticate` sets it from the key.
- A customer session (0008 §9, later) arrives with the storefront's publishable key, so
  it gets the same channel; no change then.

## 4. The storefront (CH6, CH7)

| Where | Rule |
| --- | --- |
| `GET /storefront/products/{id}` | 404 `product_not_found` unless active **and** published to the caller's channel |
| `GET /storefront/search` | only documents published to the caller's channel (CH8) |
| `POST /storefront/carts` | the cart records the caller's channel |
| Adding a line | `422 product_not_published` when the product isn't in the cart's channel |
| A cart's priced lines | a line whose product was unpublished since is `not_published` (unavailable, left out of totals), as a product removed from the catalog is `not_available` |
| Checkout | refused while any line is unavailable (`cart_unavailable`, as now) |
| The order | `channel` copied from the cart; `order.placed` gains it (an added field, so still version 1: 0016 versions renames and reshapes) |

A cart's channel is fixed: carts are created and used by one storefront. Admin routes
that touch carts (linking a customer) don't change it.

## 5. Search (CH8)

- `SearchDocument` gains `channels: Vec<ChannelId>` (the product's publications, the
  same on every variant); `build_documents` takes them from the sync's batch load
  (`channels_of`).
- `SearchQuery` gains `channel: Option<ChannelId>`: documents published to it only. The
  `search` command passes the caller's (`None` for staff, everything).
- `reference::search` filters on it; the contract's fixture and battery gain channels.
- Postgres: a `channels uuid[]` column with a GIN index, `d.channels @> ARRAY[$c]`;
  facets and totals follow the matched rows, so they need nothing else. The migration
  empties the index (the host rebuilds it at start).
- `SearchIndexer` marks the products of `channel.products_published` / `_unpublished`.
  `channel.changed` marks nothing (a name isn't in documents).

## 6. Events

| Event | Payload | When |
| --- | --- | --- |
| `channel.changed` | `channel` | created, renamed |
| `channel.products_published` | `channel`, `products` | published (staged) |
| `channel.products_unpublished` | `channel`, `products` | unpublished (staged) |
| `order.placed` | gains `channel` (additive, version 1) | checkout |

## 7. API and admin

**Admin** (`channels:*` for channels, `products:write` to publish):
- `GET /admin/channels`, `POST /admin/channels`, `GET`/`PATCH /admin/channels/{id}`.
- `POST /admin/channels/{id}/products` and `DELETE /admin/channels/{id}/products`
  (`{"products": [...]}`), for bulk publishing.
- `PUT /admin/products/{id}/channels` (`{"channels": [...]}`): a product's channels,
  replaced (publishes and unpublishes in one batch), for the product page.
- The admin product response gains `channels`; the product list a `channel` filter.
- `POST /admin/api-keys` takes `channel` for a publishable key; key responses show it.
- Orders' admin response and list show their channel.

**Admin screens:** Settings → Channels (list, create, rename, each channel's
publishable keys), a "Publishing" card on the product page (a checkbox per channel), a
channel column and filter on the product list, the channel picker when creating a
publishable key, the channel on orders.

**Storefront:** nothing new to call; responses only get narrower. The store setup
(next) starts from the Online Store channel and its key.

## 8. Changes to existing crates

- `hypha-core`: `ChannelId`; `Ctx::channel`; `Scope::ChannelsRead` / `ChannelsWrite`.
- `hypha-auth`: `ApiKey::channel`, its rules and storage column.
- `hypha-cart`: `Cart::channel`; `PricingPorts` gains `HasChannels`; `LineStatus::NotPublished`
  (as `NotAvailable`); `add_line` checks publication.
- `hypha-checkout` / `hypha-orders`: `Order::channel`, and `order.placed` gains it.
- `hypha-products`: nothing (publications live in `hypha-channels`); the merchandising
  view (`ProductView`) gains the product's channels for admin responses.
- `hypha-search-index` / `hypha-search`: section 5.
- `hypha-store-postgres`: migration (section 9), repos, the search column.
- `hypha-host-axum`: routes, `Caller` sets the channel, OpenAPI, SDK, e2e.

## 9. Migration

One migration, for existing stores:
1. `channels` and `channel_products` tables; `api_keys.channel_id` (nullable,
   referencing `channels`), `carts.channel_id`, `orders.channel_id`.
2. Insert "Online Store" (`online_store`) with a generated UUID.
3. Point every publishable key, cart and order at it; publish every product to it.
4. Make the columns `NOT NULL` where the rules require (carts, orders; keys by kind,
   with a `CHECK`).

A fresh database gets the same: an Online Store channel with nothing published and no
keys, which the store setup fills.

## 10. Build order

Each step ends green (fmt, clippy, tests, wasm, Postgres contract, e2e).

1. Core: `ChannelId`, `Ctx::channel`, the scopes.
2. `hypha-channels`: the aggregate, publications, events, commands, repo, in-memory repo
   and contract; Postgres and the migration.
3. Auth: the key's channel; `Caller` sets it on `Ctx`.
4. Carts, checkout, orders: the cart's channel, `not_published`, the order's channel
   (also on `order.placed`); the storefront product read.
5. Search: documents' channels, the query's channel, the handler, the engine column and
   contract.
6. Host routes, OpenAPI, SDK, e2e.
7. Admin: Settings → Channels, the product's Publishing card, the key's channel, orders.

## 11. Open decisions

**Q1. Where channels live.** *Recommended: A.*
- **A. A new leaf crate, `hypha-channels`** (section 2): its own aggregate and
  membership; products, carts, search and auth reach it without depending on each other.
- **B. Inside `hypha-stores`**, as store configuration: one crate less, but publications
  are product data that grows with the catalog, unlike the store's own record.

**Q2. Deleting channels.** *Recommended: not in this note.* Renaming only; a channel
with keys, carts or orders can't simply vanish. Archiving (keys stop working, products
stay published for history) comes when someone needs it.

**Q3. The scopes.** *Recommended: `channels:read` / `channels:write` (CH10).* The
alternative folds channel management into `stores:write`; separate scopes let an app
(an agentic adapter, a marketplace connector) manage its channel without editing the
store.

**Q4. Publishing on creation.** *Decided: no* (CH9). The product page's Publishing card
offers "all channels" as one click, and the store setup publishes what the merchant
picks.

## 12. Later

- The agentic channel's UCP adapter (via ucphub.ai), selling through an `agentic`
  channel's key.
- Archiving channels; scheduled publishing (publish at a date).
- Collections published per channel, once collections exist.
- Channel-specific content (titles, descriptions per channel) if marketplaces need it.
- Reporting by channel (orders, revenue).
