# 0024: Admin tables, rich text, onboarding, and the storefront on Astryx

- **Status:** Accepted (2026-10-17). Danilo's answers: descriptions stay HTML, tables
  sort and filter on the server, a setup wizard beside the checklist, the storefront
  moves to Astryx with its own theme (this replaces 0023's SF8).
- **Scope:**
  - sorting and more filters on the paged admin lists, and the lists that are missing
    (customers, staff, roles);
  - one reusable resource table in the admin, used by every list;
  - a rich text editor for HTML descriptions;
  - a first-run setup wizard;
  - a screen-by-screen pass over the admin;
  - the storefront's pages rebuilt on Astryx (product, listing and search, cart,
    checkout).
- **Not in scope:**
  - totals on paged lists (0012, L3 stands);
  - saved views, column picking and bulk edits beyond what existing set-based routes
    already do (selection is built so they can come later);
  - payments in checkout (0021, SU8);
  - admin full-text search: `q` stays a substring match, enough for SKUs, handles and
    emails.
- **Related:** 0012 (lists and pages; this extends L1/L2), 0011 (the admin), 0014
  (product editing), 0019 (search), 0021 (setup checklist), 0023 (the storefront).

## Decisions

| | Decision |
| --- | --- |
| AU1 | **Sorting is a `sort` query parameter with a fixed set of values per list**, named like the storefront's search sorts: `newest` (the default, today's order), `oldest`, `title_asc`, `title_desc`, `updated_desc`, `updated_asc`, and per list what its table shows (§2). An unknown value is `400 invalid_query`. Additive to 0012. *Hard to reverse* (the names) |
| AU2 | **Keyset pages for every sort.** The cursor (still opaque, format version 2) carries the sort, the last row's sort key and its ID; the next page is the rows after `(key, id)` in that order, with the ID breaking ties in the same direction. One index per sort key with the ID (`(lower(title), id)`), so a page stays one range scan on a large catalog. A cursor made for another sort or other filters is `400 invalid_cursor`, as now. Version 1 cursors keep working for `newest`. *Hard to reverse* |
| AU3 | **The domain sees a sort, not SQL.** Each list's sort is an enum in its domain crate (`ProductSort`), and `PageRequest` grows a generic `after` that holds the sort key with the ID (`hypha-core`'s `Keyset<K, Id>`). Adapters map each variant to an `ORDER BY`; the in-memory `Table::page` takes a key function. The contract checks each sort's order, ties and paging across them |
| AU4 | **Filters stay typed query parameters** (0012, L5), and a repeatable one (`status=active&status=draft`) means any of them |
| AU5 | **Product lists filter by category and channel without a new crate edge.** `ProductFilter` takes core's `CategoryId` and `ChannelId` as plain values; the Postgres adapter joins `category_products` and `publications`, and the in-memory ports read the shared membership. Like the routes in `error.rs` that store other domains' IDs unchecked, an unknown ID just matches nothing |
| AU6 | **Small lists sort and filter in the admin.** Lists the API returns whole and that stay small (markets, channels, zones, apps, roles, the category tree) get the same table, sorting and filtering the rows it has. No API change |
| AU7 | **One `ResourceTable` in the admin** (`components/resource-table/`): Astryx `Table` with its sort, filter, pagination and selection plugins, driven by the URL. A server page reads `searchParams`, calls the API and passes the rows and `next_cursor`; the client table changes the URL when a header, filter or page changes. Each resource declares its columns, sorts and filters once (`resources/products.tsx`) |
| AU8 | **Back and forward through pages.** The table keeps the cursors it has visited in the URL (`?cursor=` plus `?prev=`, a short stack), so Previous works without an API change. The API stays forward-only |
| AU9 | **Rich text is stored as HTML, as now.** `@astryxdesign/richtext` (Lexical) loads a description by parsing its HTML (`@lexical/html`) and saves `getHTML()`; the API still sanitises with ammonia's defaults, which keep everything the toolbar makes (paragraphs, h2–h3, lists, quotes, links, bold, italic, underline, strike, code) and drop Lexical's classes and styles. Saved HTML loads back the same, and a test pins that. The store's description stays plain text |
| AU10 | **Astryx moves to 0.7.1** for `@astryxdesign/richtext`, which requires that core version exactly. Exact pins stay |
| AU11 | **A setup wizard at `/setup`**, offered from Home while the checklist (0021, SU9) isn't done, and opened after the first login when the store has no market and no product. Each step is a screen's own form, saved as it goes: store details, a market, a shipping zone, a first product with price and stock, a channel to sell on with a publishable key. Skipping a step leaves it on the checklist. Each step is done when the checklist's data says so; the store keeps only where the merchant was (`setup.step`) and when they finished or put the guide away (`setup.finished_at`, `PATCH /admin/store` with `setup_step` / `setup_finished`), so the guide shows only until then and reopens on its step (Danilo, 2026-10-17). Stores already selling were marked finished by migration 0046 |
| AU12 | **The storefront uses Astryx with its own theme** (`apps/storefront/theme/`), built from the `product-detail`, `payment-form` (without card details), `product-gallery` and `library` templates. Pages still render on the server; only the interactive parts (variant picker, quantity, filters, the search box) are client components. Replaces 0023, SF8 |

## 1. Core

- `hypha-core/src/page.rs`:
  - `Keyset<K, Id> { key: K, id: Id }`;
  - `PageRequest<Id, K = ()>`: `after: Option<Keyset<K, Id>>`, so today's lists
    (`K = ()`) don't change;
  - `Page::from_overfetch` unchanged.
- The host's cursor (`routes/page.rs`) becomes `{v: 2, sort, key, after, filters}`; the
  key is JSON (a string, a timestamp in µs, a number), decoded by the list's sort.

## 2. Lists

| Route | New sorts | New filters |
| --- | --- | --- |
| `GET /admin/products` | `oldest`, `title_asc/desc`, `updated_desc/asc` | `status` repeatable, `category`, `channel`, `vendor`, `product_type`, `tag` |
| `GET /admin/orders` | `oldest`, `total_desc/asc` (within one currency, so only with `currency`) | `q` (number or email), `financial_status`, `fulfillment_status`, `cancelled`, `channel`, `country`, `currency`, `placed_from`, `placed_to` |
| `GET /admin/assets` | `oldest`, `name_asc/desc` | `q` (file name, alt text) |
| `GET /admin/customers` (new) | `newest`, `oldest`, `email_asc/desc`, `name_asc/desc` | `q` (email, name, phone) |
| `GET /admin/users` (new) | (unpaged, AU6: a store's staff is short) | |
| `GET /admin/roles` (new) | (unpaged, AU6) | |
| `GET /admin/invites` (new) | (unpaged; not yet accepted, expired too) | |

- Customers are a summary (0012, L7): `CustomerSummary` (email, name, phone, address
  count). Order counts would need orders, a layer the customers crate can't see; a
  later read in the host can add them. Staff lists return the users, roles and
  invites as their single reads do.
- Repository methods and contract scenarios first (CLAUDE.md), then routes, then e2e
  steps.
- Indexes, one migration per list: `(status, id)` already exists; add `(lower(title),
  id)`, `(updated_at, id)`, orders' `(placed_at, id)`, `(lower(email), id)`, and trigram
  indexes only if `q` turns out slow (not now).

## 3. The admin's table (AU7, AU8)

- `ResourceTable<Row>`: a client component.
  - Props: `rows`, `nextCursor`, the resource's `columns`, `sorts` (column key → API
    sort values), `filters` (kind: select, multi-select, text, date range, each mapped
    to a query parameter), an optional `selection` with its toolbar actions.
  - Above it: a `Toolbar` with a search box (`q`), the filters as `Selector` /
    `MultiSelector` / `DateRangeInput`, and "Clear filters".
  - Changing sort or a filter resets the cursor; paging keeps them.
  - Empty states: "Nothing yet" with the create action, apart from "No results" with
    "Clear filters".
  - Rows link to the detail page (the whole row, by keyboard too).
- Server side, one helper turns `searchParams` into the API query and handles a stale
  cursor (today each page checks for a 400 itself).
- The pickers that page with "Load more" (media, products for catalogs and categories)
  use the same table in a dialog.

## 4. Rich text (AU9)

- `components/rich-text-field.tsx` (client): `RichTextEditor` with the toolbar
  (headings h2–h3; h1 is the page's title), a hidden input carrying the HTML, so the
  existing forms and server actions don't change.
- Used for product and category descriptions. Empty editor saves `null`.

## 5. Setup wizard (AU11)

- Astryx `Stepper` in the `form-wizard-vertical` layout: steps on the left, the form,
  and a short "why" on the right.
- Steps, each done when the checklist says so:
  1. Store: name, contact email, timezone, default language.
  2. Market: name, countries, currency, prices with or without tax.
  3. Shipping: a zone covering the market's countries with one flat rate.
  4. Product: title, one price, stock at the default location, active.
  5. Sell: publish it to the online store channel and make a publishable key, shown
     with the line to paste into the storefront's `.dev.vars`.

## 6. Screen-by-screen pass

Each screen in turn, against the same list: the table (§3) where there's a list, a page
header with the primary action, inline validation from the API's error `details`,
confirmations for deletes (`AlertDialog`), toasts for saved, loading skeletons, and
empty states that say what to do next. Known gaps to close on the way:

- **The product editor first** (Danilo: creating options and variants is the biggest
  pain): two columns, the content (title, description, media, options, variants) on
  the left and the settings (status, channels, categories, organisation) on the right,
  and options and variants that are quick to add, rename, reorder and remove. Errors
  after deleting variants and adding new ones: to look into after the redo.
- Customers, Staff and API keys have no pages (the nav shows them as not ready).
- Orders: no filters; the order page needs the `detail-page` layout (header with status
  and actions, lines, totals, a timeline from the order's events).
- Media pickers page with "Load more".
- Markets: the catalog and price list cards page separately on one screen.
- Danilo's list of issues from trying it (to collect).

## 7. Storefront (AU12)

- Theme: `apps/storefront/theme/shopTheme.ts`, built like the admin's
  (`theme:build`, committed output).
- Product page from `product-detail`: gallery, option pickers as `SelectableCard`s
  (unavailable combinations disabled), quantity, add to cart, description and details in
  `Collapsible`s, "not sold in your country" from `sold_in_country`.
- Listing and search from `library` / `product-gallery`: facets as checkbox lists in a
  side panel (a sheet on phones), price range, sort, Previous/Next.
- A search box in the header with suggestions (`Typeahead`, the first five results of
  `/storefront/search`, through a server action).
- Cart and checkout from `payment-form` without the card section: contact, address,
  shipping option (`RadioList`), a sticky summary, "Place order".

**The showcase pass (Danilo, 2026-10-17):**

- **Static by default.** Every page under a country is static with ISR (`revalidate =
  300`, empty `generateStaticParams`: nothing is built ahead, since there's no API at
  build time; a page renders on its first visit and refreshes in the background), except
  search, cart, checkout, order and account, which render per request.
- **The cart lives in the browser.** A static page can't read the cart cookie, so a
  `CartProvider` reads the cart through a server action once the page is in, and the
  cart's actions answer with the cart. The header's count, a cart drawer (Astryx
  `Dialog` against the end edge) and the cart page share it and its lines.
- **Static listings, filtered in the browser.** A category page renders its first page;
  a filter, sort or page in the URL is read through a server action (`listingPage`), so
  the page stays static. Search reads its query on the server.
- **No country picker.** `proxy.ts` already finds the country (cookie, geolocation,
  the store's first); the header offers the store's currencies instead, as Shopify
  themes do, and picking one moves to a country paying in it.
- **Astryx for the frame too:** `Layout` (header, content, footer) and `TopNav` (the
  store, categories with a mega menu for subcategories, search in a `Popover`, currency,
  account, cart). Only image fills, sticky columns and the merchant's HTML have CSS.
- **Home:** a hero, category tiles, new arrivals, a shelf per top category, and the best
  prices. A "sale" shelf needs compare-at prices, which the API doesn't have yet.
- **Account** is a placeholder until customer accounts (0008 §9).

## 8. Build order

1. Core keyset paging and the cursor's version 2; product sorts and filters (repo,
   contract, Postgres, routes, e2e).
2. Order, asset filters and sorts; customer, user, role and invite lists.
3. `ResourceTable` and the products, orders, media, customers, staff and API key pages.
4. Astryx 0.7.1 and the rich text field.
5. The setup wizard.
6. The screen-by-screen pass (§6).
7. The storefront on Astryx (§7).

## 9. Hard to reverse

- **AU1, AU2:** sort names and the cursor's contents once clients page with them; the
  version in the cursor is what lets them change.
- **The new summaries** (`CustomerSummary`, `UserSummary`): fields can be added, not
  removed (0012, L7).
- **AU12:** moving the storefront to a component library is a rewrite of its markup; a
  merchant forking it inherits Astryx.
