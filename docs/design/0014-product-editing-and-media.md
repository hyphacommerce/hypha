# 0014: Product editing and product media

- **Status:** Accepted (2026-10-15). Danilo answered the open decisions in section 9
  the same day; answers are marked *Decided*.
- **Scope:** editing a product after it is created (title, handle, options and their
  values, variants: SKU, base price, selections), attaching media to products and
  variants (0013 phase 2: the `hypha-merchandising` crate, asset usages, the delete
  guard, "used by", media in product responses and list summaries), and the admin
  product page that does all of it.
- **Not in scope:** deleting products, handle redirects, bulk variant editing (CSV, a
  spreadsheet grid), variant reordering, per-usage alt text (0013 M11), media on
  collections, replacing an asset's file (`POST /admin/assets/{id}/replace`, see Q7),
  storefront product listing (0010 S5), the outbox events these edits will emit (next
  note).
- **Related:** 0001 (versions, batches), 0002 §7 (no cross-module foreign keys),
  0010 (surfaces, OpenAPI), 0011 (the admin), 0012 (lists), 0013 (media, M7 and M8).

## Decisions

| | Decision |
| --- | --- |
| E1 | **Every edit is a command on the `Product` aggregate**, loaded whole, changed through methods that keep its invariants, and written with its version: options, values and variants are children, not aggregates of their own (section 1) |
| E2 | **`PATCH /admin/products/{id}`** edits title and handle as a JSON Merge Patch; status keeps its own route (its transitions are rules, not fields) (section 2) |
| E3 | **Options are written as one list** (`PUT /admin/products/{id}/options`): a product has a handful of options and values, their order matters, and the rules that tie them to variants are easier to check on the whole list than across several calls. Existing options and values keep their IDs; new ones have none and get them. *Hard to reverse* (Q2) |
| E4 | **Variants have their own routes** (`POST`, `PATCH`, `DELETE` under `/admin/products/{id}/variants`): a variant ID is referenced by price lists, inventory, carts and orders, so a variant should only disappear when someone deletes it by name, never by being left out of a list. *Hard to reverse* (Q1) |
| E5 | **Variants stay sparse.** Nothing generates the Cartesian product of options: a merchant adds the combinations that exist. Adding an option gives existing variants its first value (Shopify's rule), which keeps every combination distinct (section 3) |
| E6 | **Removing an option value that variants use is refused** (`409 option_value_in_use`, the variants in `details`) unless the request names those variants for removal too, so one request does both atomically and nothing disappears silently. *Hard to reverse* (Q2) |
| E7 | **Option names, value labels and SKUs follow the shared text rule** (trimmed, at most 255 characters; names and labels required). A product has at most 2,048 variants, so it stays one load; the number of options isn't capped (Q5, *Decided*). A set SKU is unique across the store (Q4, *Decided*) |
| E8 | **Media attaches through `hypha-merchandising`**, a composing crate above `hypha-products` and `hypha-media` (0013 M7, *Decided*). The product stores asset IDs; merchandising checks the assets and writes the product and its usages in one batch (section 5) |
| E9 | **Usages are owned by the product's media list only**: roles `featured` and `gallery`. A variant's image is always in the list, so it adds no usage; that keeps every usage write in merchandising, and editing variants in `hypha-products` can never leave a stale one. *Changes 0013 §6*, which also listed a `variant` role; *Decided* (section 6) |
| E10 | **The delete guard is a database rule**: `asset_usages.asset_id` references `assets` (both are media's own tables, so 0002 §7 allows it). A concurrent attach and delete can't both win: the attach fails as `asset_not_found`, the delete as `asset_in_use`. The in-memory adapter enforces the same, and the contract checks it (section 6) |
| E11 | **Product responses carry media with ready URLs**: the admin and storefront product responses gain `media` and each variant `media_asset_id`; the list summary gains `featured_media`. *Hard to reverse* (Q3, section 7) |

## 1. The aggregate

`Product` gains `media: Vec<AssetId>` (ordered, first is featured) and each `Variant`
gains `media: Option<AssetId>`. New methods, each keeping every invariant `restore`
checks:

| Method | Rules |
| --- | --- |
| `rename(title)` | Shared text rule, required |
| `set_handle(handle)` | `Handle` rules; uniqueness is the repo's key, as on create |
| `set_options(Vec<OptionInput>, remove_variants: &[VariantId])` | Section 3 |
| `add_variant(NewVariant)` | Exists today; gains the SKU text rule and the 2,048 cap |
| `update_variant(id, VariantPatch)` | SKU, base price, selections; the combination stays unique |
| `remove_variant(id)` | Not the last one (`no_variants`) |
| `set_media(Vec<AssetId>)` | At most 250, no duplicates; clears variants pointing at a removed item |
| `set_variant_media(id, Option<AssetId>)` | The asset is in the product's list (`media_not_in_product`) |

`hypha-products` knows asset IDs only as plain values (the leaf rule). Its commands never
call `set_media` / `set_variant_media`; merchandising does.

**Variant selections are stored in option order.** Reordering options permutes every
variant's selections; the aggregate does it, so adapters keep storing one ordered array.

## 2. Title and handle

`PATCH /admin/products/{id}` with `{"title"?, "handle"?}` (Merge Patch; `null` refused
for both, like the store's name). Same checks as creating: `invalid_handle`,
`handle_taken` (409, also when a concurrent write takes it first), text errors (422).

Changing a handle changes storefront URLs. Redirects from old handles are *later*
(their own aggregate, so a handle can be reused once its redirect is removed); the admin
warns before saving a new handle on an active product.

## 3. Options and values

`PUT /admin/products/{id}/options`:

```json
{
  "options": [
    { "id": "opt_…", "name": "Size", "values": [ { "id": "optval_…", "label": "S" }, { "label": "XL" } ] },
    { "name": "Material", "values": [ { "label": "Cotton" } ] }
  ],
  "remove_variants": ["var_…"]
}
```

The list replaces the product's options, in order. An entry with an `id` is that
existing option or value (renamed or moved); one without is new. What happens to
variants, in order:

1. **Removed values:** a variant selecting a value that's no longer in the list is in
   `remove_variants` or the request fails with `409 option_value_in_use`
   (`details.variants`: the IDs that use it, so a client can repeat the request with
   them). Listing a variant that isn't affected is `422 variant_not_affected`, so a stale
   form can't delete the wrong thing.
2. **Removed options:** variants drop that selection. If two would then be the same
   combination, the request fails with `409 duplicate_option_combination` naming them;
   the merchant removes one first (or lists it in `remove_variants`).
3. **New options:** every remaining variant gets the new option's first value. Since the
   remaining combinations were distinct, they stay distinct.
4. **The result** needs at least one variant (`no_variants`). A product with no options
   is a single-variant product again; that only works with one variant left, which rule
   2 already enforces.

Errors: `unknown_option` / `unknown_option_value` (422, an `id` this product doesn't
have), `empty_option`, `duplicate_option_name`, `duplicate_option_value` (a label twice
in one option, compared case-insensitively), text errors.

Creating a product stays `POST /admin/products` (one variant, no options): the merchant
adds options afterwards, and rule 3 gives the first variant its values. No separate
"create configurable product" route is needed.

## 4. Variants

| Route | Body | Notes |
| --- | --- | --- |
| `POST /admin/products/{id}/variants` | `{sku?, price, selections}` | `201`, the product. One value per option, in option order |
| `PATCH /admin/products/{id}/variants/{variant}` | `{sku?, price?, selections?}` | Merge Patch; `sku: null` clears it |
| `DELETE /admin/products/{id}/variants/{variant}` | | The product; not the last variant (`409 no_variants`) |

All return the whole product (`ProductResponse`), like the status route, so the admin
re-renders from one response. Scope `products:write`.

Errors: `variant_not_found` (404, in the URL), `selection_count_mismatch`,
`unknown_option_value`, `duplicate_option_combination`, `too_many_variants` (422),
`invalid_price`, text errors (422), `sku_taken` (409: SKUs are unique when set, Q4).

**What removing a variant leaves behind.** Other domains hold variant IDs unchecked
(`error.rs`): fixed prices in price lists and inventory items keep the ID and never match
again; cart lines become unavailable (`variant_not_in_product`), as they do today when a
product is archived; orders keep their snapshot. Cleaning up fixed prices and inventory
items is the outbox's first job (next note): a `product.variant_removed` event and
handlers in those crates. Until then the admin warns before deleting a variant that has
stock.

## 5. Attaching media (`hypha-merchandising`)

A new composing crate: depends on `hypha-products`, `hypha-media` and core, nothing else.
Its commands:

```rust
set_product_media<S: HasProducts + HasAssets + HasBatch>(ctx, ports, product, Vec<AssetId>)
    where S::Batch: StageProducts + StageAssetUsages
set_variant_media<S: HasProducts + HasBatch>(ctx, ports, product, variant, Option<AssetId>)
get_product_with_media(ctx, ports, product) -> ProductView { product, assets }
list_products_with_media(ctx, ports, status, page) -> Page<ProductSummaryView>
```

- **`set_product_media`** (`PUT /admin/products/{id}/media`, `{"assets": ["asset_…", …]}`):
  `products:write`. Loads the product and the assets (`AssetRepo::get_many`, one query),
  checks each exists (`asset_not_found`, 422: an ID in the body), is `ready`
  (`asset_not_ready`), public (`asset_not_public`) and an image or video
  (`asset_kind_not_allowed`, Q6; 3D models join the allowed kinds when media supports
  them), then `Product::set_media` (`too_many_media`,
  `duplicate_media`) and stages the product and its usages in one batch. A stale write
  retries once from the load, like checkout. Removing an item clears variants that showed
  it.
- **`set_variant_media`** (`PUT /admin/products/{id}/variants/{variant}/media`,
  `{"asset": "asset_…" | null}`): only the product is written, since the list (and so
  the usages) doesn't change. Must be in the list (`media_not_in_product`, 422) and an image
  (`asset_kind_not_allowed`).
- **Reads** need only `products:read`: a product's media are part of the product, so a
  key that reads products sees their images without `media:read`.
- **Upload from the product page** is two calls the admin already has: upload (0013
  section 3, `media:write`), then `set_product_media` with the new asset appended.

`hypha-products` gains `StageProducts` (`stage_insert`, `stage_update`) next to its repo,
like `StageCarts`; `hypha-media` gains `StageAssetUsages` and an `AssetUsageRepo`.

## 6. Usages and the delete guard

```text
AssetUsage { asset: AssetId, owner: UsageOwner { kind: "product", id: "prod_…" }, role: "featured" | "gallery" }
```

- `StageAssetUsages::stage_replace(owner, Vec<AssetUsage>)`: the owner's usages become
  exactly these (wholesale, like product children). Committing fails with
  `RepoError::Missing(keys::ASSET)` (a new variant of `RepoError`, *proposed*) when one of
  the assets no longer exists; merchandising reports it as `asset_not_found`.
- `AssetUsageRepo::list(asset, PageRequest<…>) -> Page<AssetUsage>`: "used by", for
  `GET /admin/assets/{id}/usages` (`media:read`, a 0012 page, newest owner first).
  `owner.kind` stays a string (spores register their own kinds, 0013 §6); `owner.id`
  is the prefixed ID as text, so a spore's IDs needn't be UUIDs.
- **`delete_asset`** checks usages first (`409 asset_in_use`, `details: {usages: [first
  25], has_more}`; the full list is the usages route) and then deletes; if a usage
  appears in between, the foreign key refuses the delete and the error is the same. The
  file is deleted only after the row, so a refused delete leaves the file in place
  (today it goes first; the order flips).
- **Postgres** (migration 0019): `product_media (product_id, position, asset_id)`,
  `variants.media_asset_id`, `asset_usages (asset_id REFERENCES assets ON DELETE
  RESTRICT, owner_kind, owner_id, role, PRIMARY KEY (owner_kind, owner_id, asset_id))`
  with an index on `(asset_id, owner_kind, owner_id)` for the usages page. No foreign key
  from products into media (0002 §7). The same migration checks existing option names,
  labels and SKUs against E7 and fails, naming the product, if any don't pass, so
  `restore` never starts refusing stored rows.
- **Contract scenarios first** (before the adapters): `product_repo` (media and variant
  media round trip; the summary's featured item equals `Product::summary`'s), a new
  `asset_usage_repo` (replace is wholesale, pages, the delete refusal, staging for a
  missing asset fails), `asset_repo` (`get_many`), `batch` (product and usages commit
  together or not at all).

## 7. API shapes

`ProductResponse` (admin) gains:

```json
"media": [
  { "asset_id": "asset_…", "kind": "image", "mime_type": "image/jpeg",
    "url": "https://…/assets/…/1/shirt.jpg", "thumbnail_url": "https://…",
    "alt": "A blue shirt", "width": 2000, "height": 2400 }
],
"variants": [ { "id": "var_…", …, "media_asset_id": "asset_…" } ]
```

- Flat items with the values a renderer needs. `alt` is the asset's today; with a
  per-usage override later (M11) it becomes "the override, else the asset's" without a
  shape change.
- No ID of its own: an asset is in a product's list at most once, so the asset ID names
  the item.
- The storefront product response gets the same `media` and `media_asset_id` (every
  attached asset is public by E8).
- `ProductSummaryResponse` gains `featured_media`: the same item shape or `null`.
- `GET /admin/assets/{id}/usages` returns a page of `{owner: {kind, id}, role}`.
- *Follow-up (found in review):* the admin's "Used by" list reads each using product
  whole to show its title (up to 25 reads per page). A lookup made for it, such as
  usages that carry their owner's label or `GET /admin/products?ids=…` returning
  summaries, would make it one call; deferred until owners other than products exist,
  since the label's shape depends on them.

The host builds URLs (`MediaUrls`, as for assets); the views carry `Asset`s, not URLs, so
the domain never knows the CDN.

## 8. The admin product page

One page, sections that save on their own (each is one request, so a failed save never
leaves another section half-written):

- **Header:** title, status badge, the status actions (as today).
- **Details:** title and handle inputs with Save; a warning on changing an active
  product's handle.
- **Media:** a grid of thumbnails, the first marked *Featured*. Drag to reorder (with
  move-left/right buttons for keyboards); remove (detaches; the asset stays in the
  library); **Upload** (a drop zone reusing the Media page's uploader, then attach) and
  **Pick from library** (a dialog with the library's grid, images and videos, select
  several). Every change saves the whole list at once.
- **Options:** each option as a name field and its values as removable chips, add value,
  add option, move up/down, Save. Removing a value that variants use shows
  "This removes N variants: S / Red, …" and confirms, then sends them in
  `remove_variants`.
- **Variants:** the table of today plus an image column (a thumbnail; clicking it opens
  a picker among the product's media, or "none"), inline SKU and price edits saved per
  row, a delete action with confirmation (warning about stock), and **Add variant**: a
  dialog with one select per option, SKU and price. No "generate all combinations".

## 9. Open decisions

**Q1. Variant routes (E4).** *Decided: A.*
- **A. Per-variant routes** (`POST` / `PATCH` / `DELETE` per variant). Explicit deletes,
  small requests, no stale-form overwrites of other variants. Bulk creation later is
  additive (`POST …/variants/bulk`).
- **B. `PUT /admin/products/{id}/variants` with the whole list.** One request for the
  whole grid, but leaving a variant out deletes it, and two people editing different
  variants overwrite each other.
- **C. A, and `POST` takes a list** (1 to 100 variants, all or none), so adding several
  combinations is one write.

**Q2. Removing an option value that variants use (E6).** *Decided: A, refuse unless
the request names the variants to remove.*
- **A. Refuse unless named** (`remove_variants`), as in section 3.
- **B. Refuse, always:** the merchant deletes the variants first, then edits options
  (two steps, not atomic).
- **C. Cascade:** variants using the value are deleted with it (Shopify's behaviour);
  simplest for the admin, but one request can delete many variants by surprise.

**Q3. Media JSON (E11).** *Decided: A, flat items with ready URLs.* `kind` is an
open set (3D models come later, Q6): clients show what they know and skip the rest.
- **A. Flat** (section 7).
- **B. Nested:** `{"asset": {…the full AssetResponse…}}`, room for per-item fields beside
  it, but storefronts get admin fields (checksum, filename, status) or a second shape.
- **C. IDs only:** `media: ["asset_…"]`; clients fetch assets themselves (a request per
  product, and storefronts can't read assets at all today).

**Q4. SKU uniqueness.** *Decided: A, unique across the store, when set.*
- **A. Unique** (`409 sku_taken`, repo key `variant_sku`): inventory, imports and
  warehouse tools look variants up by SKU. Loosening later is easy; tightening later
  needs every duplicate fixed first.
- **B. Not enforced** (Shopify): merchants who reuse SKUs on purpose aren't blocked.

**Q5. Limits (E7).** *Decided: 2,048 variants, no cap on options* (the recommendation
was Shopify's 3 options as well). Raising a limit later is easy; lowering one is not.

**Q6. What media a product can hold.** *Decided: images and videos in the list; a
variant's media must be an image; 3D models later*, when media stores them. PDFs and other files belong on a product as downloads
or documents (later), not in its gallery.

**Q7. Replacing a file** (`POST /admin/assets/{id}/replace`, 0013 §10 put it in phase 2).
*Decided: phase 3*, with imports by URL: it needs the version bump on the stored key
and a second upload flow, and nothing on the product page needs it.

## 10. Commits (after the answers)

On `feat/product-editing`:

1. `docs: design note 0014` (with the answers).
2. `feat(products): edit title, handle, options and variants` (aggregate, commands, tests).
3. `test(testkit): contract scenarios for product media, usages and batches`.
4. `feat(media): asset usages and the delete guard` (`AssetUsageRepo`,
   `StageAssetUsages`, `get_many`, in memory and Postgres, migration 0019).
5. `feat(merchandising): attach media to products and variants` (new crate, CLAUDE.md
   layer table and wasm list).
6. `feat(host-axum): product editing and media routes` (routes, errors, OpenAPI, e2e).
7. `feat(sdk): regenerate`, `feat(admin): edit products and their media`.
