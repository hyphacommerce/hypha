# 0022: Categories

- **Status:** Accepted (2026-10-16; section 7 answered). Danilo wants categories for the first storefront,
  shaped the way most merchants already think: parent and child categories, as in
  Medusa version 1. He doesn't want Shopify's model: manual and rule-based ("smart")
  collections beside a fixed, nested product taxonomy.
- **His answers to section 7** (Q5, Q6 and CA10 came after):
  - several categories per product;
  - depth limited to 5 by default, as a setting;
  - a merchant's own product order deferred, though "many people love that";
  - an image and an HTML description per category now (descriptions matter for SEO).
- He then asked what Medusa v2 does; section 1 is now checked against its source.
- **Scope:**
  - a tree of categories;
  - products belonging to any number of them;
  - listing a category's products, its subcategories' included, through search;
  - storefront reads and admin screens.
- **Not in scope:**
  - rule-based categories;
  - a fixed taxonomy (Google's, Shopify's);
  - manual product order within a category (Q2);
  - categories per channel or market;
  - translations (0021 §5 leaves room).
- **Related:** 0019 (search documents and filters), 0020 (channels: a membership like
  publications), 0021 (store setup), 0014 (media, for a category image).

## Decisions

| | Decision |
| --- | --- |
| CA1 | **A plain tree.** A `Category` (`pcat_…`) has a name, a handle, an optional description, an optional parent, a position among its siblings, and whether it's visible. Nothing else defines it: no rules, no fixed taxonomy. *Decided* |
| CA2 | **A product can be in any number of categories, at any level.** Membership is set-based (category ↔ product), with events, like catalog membership and publications. A "Shirts" product can also be in "Sale". *Decided* |
| CA3 | **A category lists its subcategories' products too.** "Clothing" shows its shirts and trousers. Search documents carry each product's categories and all their ancestors, so one filter on "Clothing" finds them all. *Decided* |
| CA4 | **Categories don't decide who can buy.** A category's listing shows only what the shopper can already see (the key's channel, the country's market). A hidden category is left out of the storefront along with everything under it, and its products stay findable elsewhere. *Decided* |
| CA5 | **A leaf crate `hypha-categories`** on core: the aggregate, membership, the tree rules and the commands. Search reads memberships through it, as it reads channels. `hypha-merchandising` sets the image (Q5). *Decided* |
| CA6 | **Handles are unique store-wide** (`/categories/shirts`), not per parent, so a category keeps its URL when it moves. A storefront can still show nested paths using the ancestors a read returns. *Hard to reverse* |
| CA7 | **No cycles, and a configurable depth limit, 5 by default.** Moving a category under itself or one of its descendants is refused. Deleting a category with children is refused until they're moved or deleted; deleting removes its memberships, never its products. *Decided* |
| CA8 | **Categories use the products scopes** (`products:read`/`write`). They're how products are organised, like options or publishing, and need no grant of their own. *Decided* |
| CA9 | **Each category has an image and an HTML description.** The description is sanitised like a product's (0014), for category pages and SEO. The image is an asset in use, so it can't be deleted from the library; `hypha-merchandising` writes it (Q5). *Decided* |
| CA10 | **Whether a category is visible is stated when it's created**: the API has no default. The admin's form asks with a select whose first choice is "Hidden", the safer one, but the request always says which. *Decided* |

## 1. What others do

**Medusa v2, checked against its source** (the [`ProductCategory` model], the [category
repository], the [store products middlewares], the [admin guide]):

- **The model:**
  - `pcat_` IDs; `name` and `description` (both marked translatable); a unique
    `handle`;
  - `parent_category` / `category_children`;
  - `rank` among siblings;
  - `is_active` (default `false`) and `is_internal` (default `false`);
  - `external_id`, `metadata`, and a materialised path `mpath`;
  - many-to-many with products.
- **Rank** is dense, 0 to n−1 among siblings. Creating, moving or deleting a category
  shifts its siblings' ranks; the admin's "Edit ranking" mode drags and nests, saving
  on each move.
- **The tree** is stored as a materialised path (`mpath`, "a.b.c"): a subtree loads with
  a `LIKE 'a.b%'` prefix query, and a move rewrites the paths below it. The source
  shows the cost of that:
  - moving a category to the root sets its path to an empty string and leaves its
    descendants' paths stale;
  - the prefix has no separator, so `a.1%` also matches `a.10`;
  - there's no check against moving a category under its own descendant;
  - there's no depth limit;
  - restoring a soft-deleted category can collide with a sibling's rank.
- **Deleting** a category with children is refused, as CA7 does.
- **Two flags with different jobs:**
  - **`is_active`** ("Status: Active / Inactive"): whether the storefront shows it yet.
  - **`is_internal`** ("Visibility: Public / Private"): a category that is never
    public, kept for staff's own grouping.

  The store's product list only filters through categories that are active and not
  internal.
- **A category's products are its own only.** Listing products with `category_id`
  returns what's directly in that category, not its children's. A storefront wanting
  "Clothing" to include "Shirts" collects the descendant IDs itself.
- **An inactive parent doesn't hide its children** in the API: each category's flags
  are its own.
- Medusa v2 also has flat **collections** (one per product) beside categories.

**Others, from memory of their docs:**
- **Shopify:** manual and rule-based collections, plus a fixed standard taxonomy.
- **Saleor:** one category per product, plus collections.
- **Vendure:** nested collections filled by facet filters.

**What we take from Medusa:**
- the tree, the handle, sibling order and the many-to-many with products;
- refusing to delete a parent;
- new categories usually starting hidden (CA10);
- translatable name and description (0021 §5 leaves the room).

**What we do differently, and why:**
- **A category lists its subcategories' products** (CA3). Search documents carry
  ancestors, so storefronts don't assemble ID lists.
- **Hiding a category hides everything below it** (CA4). Medusa leaves that to each
  storefront.
- **The tree is a parent pointer per category, checked in memory** (`CategoryTree`, from
  one `list`), not a materialised path. Stores have tens or hundreds of categories, so
  loading them all is cheap. It also means no path column that a move can leave
  wrong, cycles and depth are checked before writing, and moving to the root is the
  same operation as any other move.
- **No soft deletion**, so no rank collisions on restore.
- **No `external_id` or `metadata`:** custom fields come with extensibility (0002), for
  every resource at once.
- **No second grouping** (collections) beside categories.

**Not taken either:** Medusa's `is_internal`, a category staff use to organise products
that shoppers never see (Q6: nobody uses it).

[`ProductCategory` model]: https://github.com/medusajs/medusa/blob/develop/packages/modules/product/src/models/product-category.ts
[category repository]: https://github.com/medusajs/medusa/blob/develop/packages/modules/product/src/repositories/product-category.ts
[store products middlewares]: https://github.com/medusajs/medusa/blob/develop/packages/medusa/src/api/store/products/middlewares.ts
[admin guide]: https://docs.medusajs.com/user-guide/products/categories

## 2. Model

**`Category` (aggregate, `pcat_…`)**
- `name`: the shared text rule.
- `handle`: the product handle rule, unique across categories (`keys::CATEGORY_HANDLE`).
- `description`: optional, sanitised HTML like a product's (0014), the same type
  (`Description`) and limits.
- `image: Option<AssetId>`: an image from the media library, in use while set. Set
  through `hypha-merchandising` (CA9, Q5).
- `parent: Option<CategoryId>`.
- `position`: among its siblings. Moving or reordering renumbers the siblings in one
  batch.
- `visible: bool`: `false` hides it and everything below it from the storefront.
  Required when creating (CA10).
- The depth limit comes from `CategorySettings::max_depth` (default 5, read from
  `[categories] max_depth` in `hypha.config.toml`, 0017; checked to be 1 to 10).
  Lowering it doesn't touch existing categories: it applies to creating and moving.
- Versioned and stamped. Records `category.changed` and `category.deleted`.

**Membership**
- `(category, product)` pairs, written set by set in batches:
  - `add_products` and `remove_products`;
  - `set_product_categories` for a product page.
- Events `category.products_added` / `category.products_removed`, as for publications.
- `categories_of(products) → map product → categories`: one query for a page or a sync
  batch.

**The tree**
- `CategoryRepo::list()` returns every category. A store has tens or hundreds, not
  millions.
- `CategoryTree`, a read model built from that list, answers parent, ancestors,
  descendants and depth.
- Commands check the tree rules on it before writing.

## 3. Search (CA3)

- `SearchDocument` gains `categories: Vec<CategoryId>`: the product's categories and
  all their ancestors, sorted.
- `SearchQuery` gains `category: Option<CategoryId>`, and `reference::search` keeps
  documents carrying it.
- **Postgres:** a GIN-indexed `categories uuid[]`, as with channels. The migration
  empties the index for the host to rebuild.
- **The `SearchIndexer` marks:**
  - membership events: their products;
  - `category.changed` when the parent or visibility changed: every product in that
    subtree, since their ancestor lists changed;
  - renames: nothing (names aren't in documents).
- `GET /storefront/search` takes `category` (an ID). A hidden category is 404 there.
- A category facet ("Shirts 12, Trousers 8") can come later: the documents already hold
  what it needs.

## 4. Routes

**Admin** (products scopes):
- `GET /admin/categories`: the whole tree, nested.
- `POST /admin/categories`.
- `GET`, `PATCH` and `DELETE /admin/categories/{id}`. PATCH takes `parent` and
  `position` to move or reorder.
- `POST` and `DELETE /admin/categories/{id}/products`: bulk membership.
- `GET /admin/categories/{id}/products`: paged.
- `PUT /admin/products/{id}/categories`: a product's categories, replaced.
- The admin product response gains `categories`, and the product list a column.

**Storefront:**
- `GET /storefront/categories`: the visible tree, nested.
- `GET /storefront/categories/by-handle/{handle}`: the category with its ancestors (a
  breadcrumb) and visible children. A hidden category is 404.
- Products come through `GET /storefront/search?category=…` (section 3), with facets,
  sorting and paging as any search.

## 5. Admin screens

- **Products → Categories:** the tree, with indentation and move up/down. Dragging can
  come later. A category page: name, handle, description, parent, visible, and its
  products.
- **The product page:** a "Categories" card, a multi-select over the tree showing paths
  ("Clothing › Shirts"). Under 0021's save bar it's part of the draft, unlike
  publishing, which saves at once. One `PUT` on Save.

## 6. Build order

1. Core: `CategoryId` (`pcat`).
2. `hypha-categories`: the aggregate, `CategoryTree`, membership, events, commands,
   the in-memory repo and the contract. Then Postgres and the migration.
3. Routes, OpenAPI, SDK, e2e.
4. Search: documents' categories, the query filter, the handler, the engine column and
   the contract.
5. Admin: the Categories pages and the product's card.

## 7. Open questions

**Q1. Several categories per product, or one?** *Decided: several.*

**Q2. A merchant's own product order within a category.** *Decided: later.* Danilo:
"many people love that". Listings sort by search's sorts until then. When it comes,
the membership rows gain a position. That's why membership is its own set of
(category, product) rows, not an array on the product. The open part is what
"manual" means in a parent category that shows its children's products.

**Q3. The depth limit.** *Decided: a setting, 5 by default* (section 2).

**Q4. A category image.** *Decided: now* (CA9).

**Q5. Who writes the image's asset usage.** *Decided: `hypha-merchandising`,* the
crate that already brings products and media together. Setting a category's image
stages the category and its asset usage in one batch, as attaching product media does.
`hypha-categories` stays a tree-and-membership leaf. This adds a merchandising →
categories edge (Danilo agreed).

**Q6. Staff-only categories (Medusa's `is_internal`).** *Decided: no.* Danilo: nobody
uses it. Hidden categories and product tags cover it.
