# 0012: Lists and pages

- **Status:** Built (2026-10-15, branch `feat/admin-lists`); the shape is a proposal for
  Danilo to confirm before merging, since clients generate from it (see §5).
- **Scope:** how list endpoints page through results, the first two lists (products,
  orders), and renaming the store, which the first admin pages need.
- **Not in scope:** search and full-text filters, sorting by anything but creation,
  lists for the other aggregates (markets, customers, staff: same shape when needed),
  storefront product listing (it needs prices per market, 0010 S5), counts.
- **Related:** 0010 (admin API, OpenAPI), 0011 (the admin, which lists first),
  0001 (repositories), CLAUDE.md (large catalogs).

## Decisions

| | Decision |
| --- | --- |
| L1 | **Cursor pages, newest first.** IDs are UUIDv7, so ordering by ID is ordering by creation; a page asks for items below the last ID it saw. One indexed range scan, and items added while paging don't shift pages the way offsets do |
| L2 | **Opaque cursors.** `?cursor=` takes the previous page's `next_cursor`. Since 0016 (section 9) it is base64url JSON of a format version, the last item's ID and the request's filters; clients must not read it, so a later sort (by title, by update) can put its sort key in the cursor without an API change. A cursor the list didn't hand out, or made with other filters, is `400 invalid_cursor`. *Hard to reverse* |
| L3 | **Response shape `{items, has_more, next_cursor}`.** `next_cursor` is `null` on the last page. No total count: counting a large catalog is a full scan on every page. *Hard to reverse* |
| L4 | **`limit` from 1 to 100, default 50.** Out of range is `422 invalid_limit`, not clamped, so a client asking for 500 finds out instead of silently getting 100 |
| L5 | **Filters are typed query parameters**, refused when unknown (`400 invalid_query`), like unknown body fields. Products filter by `status` |
| L6 | **Lists live in the repository port** (`ProductRepo::list`, `OrderRepo::list`) taking `PageRequest<Id>` and returning `Page<T>` from `hypha-core`; adapters fetch `limit + 1` rows so `has_more` costs nothing extra |
| L7 | **Lists return summaries, not aggregates**, from the repository itself: `ProductSummary` (handle, title, status, variant count, a price range per currency) and `OrderSummary` (number, date, email, country, statuses, total, item count). A table needs no variants or lines, and with large catalogs a page of whole products could be megabytes. Each domain defines its summary next to the aggregate (`Product::summary`); adapters may compute it in storage, and the contract checks the two agree. The same for every later list. *Hard to reverse* (the item shapes) |

## 1. Core types (`hypha-core/src/page.rs`)

- `PageLimit`: 1..=100, `DEFAULT` 50; `PageError::LimitOutOfRange` (`invalid_limit`).
- `PageRequest<Id> { after: Option<Id>, limit: PageLimit }`: the domain sees IDs, not
  cursors. The host turns a cursor into `after`.
- `Page<T> { items, has_more }`, built by `Page::from_overfetch(items, limit)`.

## 2. Storage

- **Postgres:** one query per page: `… WHERE (filter) AND ($after IS NULL OR id < $after)
  ORDER BY id DESC LIMIT n + 1`, with the summary's aggregates (variant count, price
  ranges, item count) in per-row subqueries, which run only for the rows the limit keeps.
  No child rows leave the database. The product status filter has an index on
  `(status, id)` (migration 0017).
- **In memory:** `Table::page` does the same over its rows.
- **Contract:** `product_repo` and `order_repo` check order, cursors (including one that
  no longer exists), exact-limit pages, filters, and that each summary equals the stored
  aggregate's (a product priced in two currencies, an item count over 2³²).

## 3. HTTP

| Route | Scope | Notes |
| --- | --- | --- |
| `GET /admin/products?status&cursor&limit` | `products:read` | `ProductSummaryPage` of `ProductSummaryResponse` |
| `GET /admin/orders?cursor&limit` | `orders:read` | `OrderSummaryPage` of `OrderSummaryResponse` |
| `PATCH /admin/store` | `stores:write` | `{"name"}`, Merge Patch; `null` refused |

- Every list shares `routes/page.rs`: `ListQuery<Filters>` reads `cursor` and `limit`
  apart from the list's own filters (which refuse unknown parameters), after
  authentication like `JsonBody`, so a bad token is a 401 whatever the query says.
  `PageParams` documents the two parameters from `PageLimit`'s constants, and
  `PageResponse<Item>` is the response, named after its items in the documents
  (`ProductSummaryResponse` → `ProductSummaryPage`).
- The admin product response grows `options` and, per variant, `sku`, `title` ("M /
  Blue") and `selections`: what a product page shows. Additive.

## 4. What others do

- **Stripe:** `limit` (1–100, default 10), `starting_after=<id>`, `{data, has_more}`;
  only creation order. IDs as cursors work because it never sorts otherwise.
- **Shopify:** opaque cursors (`pageInfo.endCursor`, `hasNextPage`) on every connection,
  any sort key.
- **We take:** Stripe's simplicity (one order, `has_more`, a hard limit) with Shopify's
  opaque cursor, so sorting can come later.

## 5. Hard to reverse

- **L2, L3:** parameter and field names and the cursor's opacity, once clients page
  with them. The alternative considered for L2, IDs as cursors (`?after=<id>`), is
  simpler to read but rules out other sort orders without a breaking change.
- **Newest first as the only order:** adding sort orders later is additive (a `sort`
  parameter with the current order as default).
- **L7, the summaries:** fields can be added later, not removed. A product's price
  ranges are a list (one per currency) because nothing yet stops variants of one
  product being priced in different currencies; one range would have to pick.
