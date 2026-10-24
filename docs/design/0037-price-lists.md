# 0037: Price lists: sales, schedules, compare-at, tiers and group prices

- **Status:** Accepted (2026-10-24): Danilo accepted the proposals, with discounts additive
  within a class (0038 DS5). §11 lists what was open.
- **Scope:** everything that sets the *unit price a shopper sees before the cart*.
  - Price lists become named, kinded (`sale` / `override`), scheduled and stackable on
    a catalog.
  - Compare-at prices on variants and on fixed prices.
  - Quantity price breaks.
  - Prices for customer groups.
  - The read model that carries all of this to carts, product pages and search.
  - The admin's price list section and its spreadsheet editor, with CSV.
- **Not in scope:**
  - Discounts: conditional, cart-level reductions with codes and combinations are
    0038. Layer (a) here, layer (b) there.
  - Gift cards, which are payment (0038 §12).
  - Exchange rates and converted-price rounding (FX stays a port, later).
  - B2B companies and quantity rules (min / max / increment).
  - Prices per channel.
  - Lowest-prior-price history for the EU Omnibus rule (reserved, §8).
- **Related:**
  - 0002 (pricing first sketched).
  - 0019 (search documents priced per market).
  - 0020 (channels: no prices per channel).
  - 0024 (a sale shelf needs compare-at).
  - 0032 §6 (prices per group, revised here, §6).
  - 0038 (discounts, built on this note's read model).

## Why

Today a market's catalog points at no price list or at one, and a price list is a
currency, one percentage and fixed per-variant prices. That's enough for "Switzerland
pays +12% in CHF" and nothing else merchants ask for on day one:

1. **Sales.** "20% off the grow kits until Sunday", shown on the product page and in
   listings with the old price struck through. Today a merchant edits base prices by
   hand and has nothing to strike through.
2. **Clearance.** It's the same thing as a sale, scoped to a category and without an end
   date.
3. **Scheduling.** "New prices from 1 January", or Black Friday that starts and ends on
   its own.
4. **Volume prices.** "€8 each from 10": the commonest wholesale price, which every
   B2B-capable platform puts on the price, not on a discount.
5. **Group prices.** A `wholesale` group sees its own prices once signed in. 0032 §6
   promised this once accounts existed (0034 is built).
6. **Editing at scale.** One dialog per variant doesn't work for a catalog of
   thousands. Merchants expect a grid and CSV.

## Decisions

| | Decision |
| --- | --- |
| PL1 | **A price list gets a `name`** (unique, ignoring case), a description, a **kind** (`sale` or `override`), a **status** (`draft` / `active`) and an optional **window** (`starts_at`, `ends_at`). The kind decides the compare-at price, not precedence (§3) |
| PL2 | **A catalog holds any number of price lists, and the lowest price wins.** Every live list that prices the variant is a candidate; the shopper pays the lowest. The base price is a candidate only when no live list prices the variant. This is Shopify's and commercetools' rule, and 0032 §6 already chose it for groups (§2) |
| PL3 | **An adjustment can be scoped**: `applies_to` all products in the catalog, or listed products, or listed categories (with their descendants). Fixed prices always apply. "20% off Grow kits" is a price list, so it shows on product pages and in search (§4) |
| PL4 | **Compare-at prices** on variants (the merchant's "was" price, in the base currency) and on fixed prices. A `sale` list's price is compared against the regular price; an `override` list's against its compare-at. A compare-at price is shown only when it's above the price (§3) |
| PL5 | **Quantity price breaks** on fixed prices: up to 10 `(minimum quantity, price)` pairs per variant, counted per variant within a cart line, as Shopify B2B does. Each candidate list is evaluated at the line's quantity, then the lowest wins (§5) |
| PL6 | **Group prices live on the list's audience**, not on a catalog context: a list may name customer groups, and then prices only buyers in one of them. A shopper's group lists compete with the public ones under PL2. This revises 0032 §6; catalog contexts stay what decides *availability*, for B2B later (§6) |
| PL7 | **Schedules need no clock in the price**: pricing filters lists by the request's time. A job step notices when a list's window opens or closes and records `price_list.started` / `.ended`, so search re-syncs and storefronts revalidate (§7) |
| PL8 | **One read model, `ResolvedPrice`** (price, compare-at, the list that won, the breaks), returned by `resolve_price` and carried by cart lines, offers and search documents. Orders copy the compare-at and the winning list (§8) |
| PL9 | **The admin edits prices in a grid** (products × variants, inline, keyboard, paste, fill, bulk "set to" / "% of base"), with CSV export and import that are previewed before they're applied. The API stays JSON bulk: CSV is the admin's (§10) |

## 1. What others do

The research was done on 2026-10-24 against current docs and source.

- **Shopify** ([PriceList](https://shopify.dev/docs/api/admin-graphql/latest/objects/PriceList),
  [catalogs](https://shopify.dev/docs/apps/build/markets/new-markets/catalogs),
  [quantity pricing](https://help.shopify.com/en/manual/b2b/catalogs/quantity-pricing)).
  - **The price list:** `name` (unique), `currency`, and a parent adjustment (a
    percentage increase or decrease).
    - `compareAtMode`: `ADJUSTED` adjusts the variant's compare-at with the list;
      `NULLIFY` hides it.
    - Fixed prices with their own `compareAtPrice`.
    - Up to 10 quantity price breaks per product, plus quantity rules (B2B).
    - Changing a list's currency deletes its fixed prices.
  - **Catalogs:** market, company location (B2B) and app catalogs, each with a
    publication and/or a price list.
  - **Resolution:** a fixed price beats the adjusted price, which beats the base price.
    Across catalogs only the most specific level counts (company location, then
    markets, then apps), and within a level **the lowest price wins**.
  - **A sale** is `price` lowered with `compareAtPrice` set. It isn't a discount and
    takes no part in combinations.
  - **Editing:** a catalog's "Manage products and pricing" page, CSV per catalog;
    `priceListFixedPricesAdd` / `…ByProductUpdate` for bulk.
  - **Merchants complain about:**
    - Lowest-wins surprises them.
    - There's no admin UI for compare-at per market.
    - Fixed prices are lost on a currency change.
    - The rounding rules aren't published.
- **Medusa v2** ([price calculation](https://docs.medusajs.com/resources/commerce-modules/pricing/price-calculation)).
  - **The price list:** `title`, `status` (`draft`/`active`), `type` (`sale` /
    `override`), `starts_at` / `ends_at`, and rules such as `customer.groups.id`.
  - **Prices** carry `min_quantity` / `max_quantity` (tiers) and their own rules.
  - **Selection:** a price-list price beats a non-list price, then the most specific
    (most rules matched) wins, then the cheapest.
  - **`calculated_price` vs `original_price`:** a `sale` list keeps the base as the
    original (a strikethrough); an `override` list replaces both.
  - **Lists hold absolute prices only:** no "−10% of base", which the admin computes
    instead.
  - **The admin's DataGrid** (products × currencies, keyboard, multi-cell fill) is the
    editor to match.
- **commercetools** ([pricing overview](https://docs.commercetools.com/api/pricing-and-discounts-overview)).
  - **Scoped prices:** country, customer group, channel, validity dates and tiers.
  - **Selection is a fallback ladder** from most specific to unscoped. Time-bound
    beats open-ended, and among equals the cheapest wins.
  - **Tiers** apply to the whole line once its minimum is reached, per line, not across
    the cart.
  - **Product discounts** (one per price, precomputed into `price.discounted`) are its
    "sale".
- **BigCommerce** ([price lists](https://docs.bigcommerce.com/docs/store-operations/pricing/price-lists)).
  - **Records:** per variant and currency, `price`, `sale_price`, `retail_price` and
    bulk tiers (fixed, price or percent).
  - **Assignment:** a list is assigned to a channel, a customer group or both.
  - **Bulk upsert:** 1,000 records per call.
- **Saleor** has no price lists: prices live on channel listings, and sales are
  catalogue promotions precomputed into a `discounted_price` (with `prior_price` kept
  for Omnibus). Group pricing needs another channel. It's the weakest here.

What we take:
- Shopify's resolution rule (lowest wins) and fixed-over-adjusted precedence.
- Medusa's `sale` / `override` kinds, its status and window, and its grid.
- Shopify's per-variant breaks and compare-at modes.
- commercetools' "time-bound lists are ordinary lists that switch on and off".

What we refuse:
- Saleor's async precompute lag. Prices are resolved at read time; search, which is
  precomputed, re-syncs on events (§7).
- Medusa's "absolute prices only". We keep adjustments, and scope them (PL3).

## 2. Several lists on a catalog, and who wins (PL2)

- **`catalog_price_lists`** replaces `catalogs.price_list_id`. A catalog holds a *set*
  of lists, written as a set like catalog products, with events. A list may sit on
  several catalogs, as today (one list for every euro market).
- **Every list on a catalog is in the market's currency.** `assign` refuses another
  currency, as `assign_price_list` does today. A list's currency still can't change.
- **A list is live** at time `t` when:
  - its status is `active`,
  - `starts_at <= t` (or none) and `t < ends_at` (or none),
  - and its audience includes the buyer (§6; an empty audience includes everyone).
- **A list prices a variant** when it holds a fixed price for it, or its adjustment's
  `applies_to` covers the variant's product. Existing lists migrate to `applies_to:
  all` with their percentage (0 included), so they keep pricing every product. A new
  list starts with no adjustment and prices only its fixed variants, which is what lets
  a sale list hold just the sale items.
- **Resolution** for a variant, a quantity and a buyer:
  1. Candidates are every live list on the catalog that prices the variant. Each one
     gives one price at the quantity: its best break at or below the quantity, else its
     fixed price, else the base price adjusted.
  2. With no candidate, the base price is the price. It must be in the market's
     currency, as today: `price_currency_mismatch`.
  3. The **price** is the lowest candidate. Ties go to the list with the earliest
     `created` (its UUIDv7 ID), so the winner is deterministic and an old list beats a
     copy of it.
  4. The **regular price** is the lowest candidate from `override` lists, or the base
     price when there's no override candidate. A sale list can't set the price a sale is
     measured against.
- **Why lowest wins, not priority.**
  - Merchants never have to manage an order. A wholesale list can't accidentally
    *raise* a group's price above a public sale: the buyer gets whichever is lower.
  - commercetools' history (sort orders, then a stop flag, then groups to fix it) is
    the cost of priorities.
  - **The surprise Shopify merchants report is real**: a markup list (`override`, +20%)
    can't sit beside a "regular" list on the same catalog and win. The answer is that a
    markup belongs on the market's catalog alone, and only lists meant to compete
    belong together. The admin says so where a list is attached (§10).

## 3. Kinds and compare-at prices (PL1, PL4)

- **`Variant.compare_at_price: Option<Price>`**, in the variant's base currency, set and
  cleared with the base price (`PATCH` merge rules). It must be above the base price,
  else `422 compare_at_not_above_price`. On the base price alone (no list), it's shown
  as the variant's "was" price.
- **`FixedPrice.compare_at: Option<Price>`**, same rule against that fixed price.
- **`PriceList.compare_at: adjusted | hidden`** (Shopify's `compareAtMode`), for
  `override` lists only:
  - `adjusted` (the default) shows the variant's compare-at with the list's adjustment
    applied.
  - `hidden` shows none unless the fixed price has its own.
- **What the shopper sees as compare-at**, for the winning candidate:

  | Winner | Compare-at |
  | --- | --- |
  | a `sale` list | its fixed price's `compare_at` if set, else the **regular price** (§2) |
  | an `override` list | its fixed price's `compare_at` if set, else the variant's compare-at, adjusted when the list says `adjusted` |
  | the base price | the variant's `compare_at_price` |

  In every case it's shown only when it's above the price. Otherwise it's `None` and
  there's no strikethrough.
- **`on_sale`** in the read model means "a compare-at is shown". It's what search
  facets and the storefront's sale shelf use.
- **Why a kind and not just compare-at fields.** The commonest sale ("20% off these")
  has no compare-at to type: the regular price *is* the compare-at, and only the list
  knows it's a sale. Medusa's distinction is exactly this.
- **Clearance** is a `sale` list with `applies_to` a category and no `ends_at`. The
  admin offers it as a template (§10). There's no separate concept.

## 4. Scoped adjustments (PL3)

- **`PriceList.adjustment: Option<Adjustment>`** where
  `Adjustment { percent: BasisPoints, applies_to: AppliesTo }` and `AppliesTo` is one of
  `All`, `Products(Vec<ProductId>)` or `Categories(Vec<CategoryId>)`. Lists of products or
  categories are capped at 1,000.
- **Categories count their descendants**, like search's category filter. The pricing
  loader reads the products' category memberships with the category tree (one query,
  pipelined with the rest).
- **Rounding** stays `Money::adjust` (half away from zero, in minor units). Price
  endings (`.99`, `.95`, whole units) are a plausible next field
  (`rounding: none | ending(n)`), reserved and not built: they matter most with FX,
  which isn't here.
- `pricing` stays a pure resolver: `PricingContext` gains the lists, their fixed
  prices and breaks, the products' categories (with ancestors) and the buyer's groups,
  all loaded by `load_pricing` / `load_pricing_for`. Carts, offers and search keep
  sharing it, so they still can't disagree.

## 5. Quantity price breaks (PL5)

- A fixed price may carry `breaks: [{min_quantity, price}]`:
  - up to 10 per variant;
  - `min_quantity` from 2 up, strictly increasing;
  - each price below the one before it (`price_breaks_not_descending`).
- A break needs a fixed price: the unit price below the first break is that fixed
  price. Breaks on adjusted prices (percent per tier) are left out. BigCommerce has
  them, and nobody else in the survey does.
- **Counted per variant within a line**, which in Hypha is the same thing since a cart
  holds one line per variant. "10 across colours" is a discount (0038's tiers), not a
  price break.
- **Where they appear:**
  - The cart prices the line at its quantity.
  - Offers (`VariantOffer`) and the storefront product response list the breaks the
    winning list has ("€8 each from 10"), computed for the shopper's audience.
  - Search indexes the quantity-1 price.
- **Quantity rules** (minimum, maximum, increment) are B2B and left for later. The read
  model reserves a place for them beside the breaks.

## 6. Group prices (PL6)

- **`PriceList.audience: Audience`**, with `Audience { groups: Vec<CustomerGroupId> }`
  (empty = everyone), stored as `price_list_groups`.
  - It's a struct and not a bare list so channels can join it later without reshaping
    the field.
  - Deleting a group that a list names is refused (`group_in_use`), as 0032 §6 planned.
- **The buyer** is the cart's or session's customer, and their groups come from
  `hypha-customers`. A cart without a customer, or a static page, is "everyone".
- **Revises 0032 §6.** That section put groups in a `CatalogContext::CustomerGroup` and
  resolved the group's catalog before the market's.
  - With several lists per catalog and lowest-wins (PL2), an audience on the list gets
    the same result (several groups: the lowest price) with one place to look and no
    second catalog to keep in step.
  - It also makes a group's sale and the public sale compete naturally.
  - `CatalogContext` stays for *which products* a context sells: B2B company locations
    later, where availability does differ.
- **The storefront's static pages show public prices.** Product and category pages are
  ISR, shared by every shopper, and search documents are per market.
  - A signed-in shopper's group price appears in the cart (always correct) and on the
    product page through a small dynamic read: the `loadCart`-style action that asks
    for the variant's offer with the session.
  - Listings show public prices. Shopify B2B has the same split. The storefront's docs
    say so where they describe prices.

## 7. Schedules (PL7)

- Pricing takes the request's time (`clock.now()`), so a cart and a product page priced
  at 00:00:01 see the sale that started at 00:00:00. Nothing has to run for prices
  themselves to be right.
- **What does have to run** is everything precomputed or cached:
  - search documents (a product's listing price, `on_sale`);
  - the storefront's ISR pages and `unstable_cache` data;
  - apps' webhooks ("a sale started").
- **A job step, `price_list_schedule`** (in `jobs.rs`, natively a job, on Workers the
  minute cron):
  - It reads lists whose `starts_at` or `ends_at` fell between its last run and now.
    The last run is kept in a one-row `price_list_schedule` table, so a missed tick
    catches up rather than skipping.
  - It records `price_list.started` / `price_list.ended { price_list, catalogs }` in the
    outbox.
  - Search's `SearchIndexer` marks the catalogs' markets' affected products (all of
    the catalog's products for `applies_to: all`, else the listed products and the
    categories' members, else the fixed variants' products).
  - The storefront's revalidation route expires the prices tag on those events, as it
    already does on `price_list.changed`.
  - `OutboxRepo::next_due`'s Workers marks gain the next window boundary, so a quiet
    store's ticks still don't touch Postgres.
- A window boundary is accurate to the job's tick (a minute) for search and pages, and
  exact for carts and checkout.

## 8. The read model (PL8)

```rust
pub struct ResolvedPrice {
    pub price: Price,
    /// Shown struck through; only when above `price`.
    pub compare_at: Option<Price>,
    /// The list whose candidate won; `None` for the base price.
    pub price_list: Option<PriceListId>,
    /// Breaks of the winning list, for showing; empty without.
    pub breaks: Vec<PriceBreak>,
}
```

- `resolve_price(&ctx, product, variant, base, compare_at, quantity, at)` returns it.
  `price_line` and `price_product` pass it on, so `LineStatus::Priced` gains
  `compare_at` and `price_list`.
- **Storefront DTOs:**
  - Variants gain `compare_at_price`, `on_sale` and `price_breaks`.
  - Listings (search) gain `compare_at` beside `listing_price` (min and max of the
    variants shown), plus an `on_sale` facet and filter.
  - Carts' priced lines gain `compare_at_price`.
- **Orders** copy `compare_at_price` and `price_list` onto each `OrderLine`. That's for
  emails ("you saved"), reports and refunds, never for totals. Old orders read `None`.
- **Search documents** gain `compare_at: Option<Money>` and `on_sale: bool`.
  `reference::search` and the `search_index` contract filter and facet on `on_sale`.
- **The EU Omnibus rule** (show the lowest price of the last 30 days during a
  reduction) needs price history per market and variant. That's reserved, not built:
  - The DTOs leave room for `lowest_prior_price`.
  - A later `price_history` table would be written by the same job step on window
    boundaries and by fixed-price events.
  - Until then the admin's sale template says merchants in the EU must set the
    compare-at price to that lowest price themselves.

## 9. API

- **Price lists:**
  - `POST` / `PATCH /admin/price-lists` gain `name`, `description`, `kind`, `status`,
    `starts_at`, `ends_at`, `adjustment {percent_bps, applies_to}`, `compare_at`
    (`adjusted`/`hidden`) and `audience {groups}`. `name` is required on create.
  - A migration names existing lists after their first market (`"<Market> prices"`),
    else `"Price list <n>"`.
  - `GET /admin/price-lists` filters by `kind`, `status` (`draft`, `scheduled`,
    `active`, `expired`: derived from status and window), `market` and `group`, and
    sorts by name or newest.
  - `DELETE /admin/price-lists/{id}`, refused while a catalog holds the list
    (`price_list_in_use`).
- **Fixed prices:** `PUT /admin/price-lists/{id}/fixed-prices` entries gain
  `compare_at` and `breaks`.
  - Capped at **1,000 entries** per request (`too_many_prices`, 422). Bulk callers page
    through.
  - Every entry's problems are answered at once, with their index (`details.entries[]`),
    as `check_fixed_prices` does for currencies.
  - `GET …/fixed-prices` filters by `product`, `category` and `q`, for the grid.
- **Catalogs:** `PUT /admin/catalogs/{id}/price-lists {price_lists: [...]}` (the set)
  and `POST` / `DELETE` for one, **replacing** `PUT …/price-list`.
  - That's a breaking change, made before the first release. The PR carries
    `breaking-api`.
- **Variants:** `compare_at_price` on product and variant writes and reads.
- **Pricing read:** `GET /admin/products/{id}/prices?market=&group=&quantity=` shows
  each variant's `ResolvedPrice` per market, with every candidate and why it lost. It
  powers the product page's "Prices" card and lets an agent answer "what does a
  wholesale buyer in CH pay for this?".
- **Storefront:** the fields in §8. A product read sent with a customer session prices
  for that customer's groups.
- **Scopes:** unchanged: `pricing:read` / `pricing:write`, with `markets:write` for
  attaching to a catalog, as today.
- **Events** (version 1, new): `price_list.created`, `.deleted`, `.started` and `.ended`;
  `catalog.price_lists_added` / `_removed`. `price_list.changed` lists the new fields
  it changed.

## 10. The admin (PL9)

- **Navigation:** "Price lists" stays under Markets but becomes a full section.
  - **The list page:** name, kind, status badge (draft / scheduled / active / expired),
    window, markets, audience and the number of prices.
  - **"New price list"** offers templates:
    - **Sale**: `sale`, an adjustment, a window.
    - **Clearance**: `sale`, a category, no end.
    - **Market prices**: `override`.
    - **Wholesale**: `override`, a group, breaks.
- **A list's page:**
  - **Settings:** name, kind, schedule, adjustment and scope, compare-at mode, audience.
  - **Where it applies:** catalogs and markets, attached and detached here. This is
    where the lowest-wins note sits.
  - **Prices:** the grid.
- **The grid:**
  - **Rows** are products with their variants beneath.
  - **Columns:** base price, compare-at, the list's computed price (greyed when it
    comes from the adjustment), the fixed price, the fixed compare-at, and a breaks cell
    that opens a small editor.
  - **Editing:**
    - Inline, with keyboard navigation and multi-cell selection.
    - Paste from a spreadsheet, and fill down.
    - Bulk actions on a selection: set to an amount, set to −x% / +x% of base, clear.
    - Filters: category, product search, "fixed only".
  - **Changes stay local until "Save"**, which sends one `PUT` per 1,000 changed
    entries plus one `DELETE` for cleared cells, and shows each failing row's error
    in place.
- **CSV:**
  - **Export** is the list's prices, one row per variant: handle, SKU, variant ID,
    options, base price, fixed price, compare-at, `break_<n>_qty` / `break_<n>_price`.
  - **Import** parses on the admin's server, matches rows by variant ID, else SKU, else
    handle and options, and shows a preview before anything is written: rows changed,
    unchanged, unmatched and invalid.
  - Applying the import sends the same `PUT`s as the grid.
- **The product page** gains a "Prices" card: the variant's price in each market, with
  the list that wins and any sale, read from `GET /admin/products/{id}/prices`. The
  variants section gains the compare-at column.
- **The storefront** shows compare-at prices struck through (product page, cards, cart
  lines), "€8 each from 10" under the price, and a "Sale" shelf and filter from search's
  `on_sale`.

## 11. Hard to reverse, and still open

- **Lowest wins across a catalog's lists (PL2).** It's a contract with merchants and
  apps. A priority could be added later as a tie-break, but not replace it without
  re-pricing stores.
- **The group audience on lists (PL6), revising 0032 §6.** This is the one change
  against an accepted note. *Open: confirm.*
- **`catalog_price_lists` replacing `catalogs.price_list_id`** (storage and the
  catalog route), before the first release.
- **Compare-at semantics (§3)**: shown only when above the price, `sale` measured
  against the regular price.
- **Order lines gaining `compare_at_price` and `price_list`.** These are additive.
- *Open:* should `draft` exist, or is a list without catalogs "draft enough"? Medusa
  and Shopify differ. The proposal keeps `draft`, since a scheduled list on a catalog
  needs a way to be paused.

## 12. Build order

Each step is a commit series with tests (resolution rules as unit tests on
`resolve_price`, one rule each, boundaries at window edges and break thresholds), routes,
OpenAPI, SDK and e2e steps:

1. **The model:**
   - Name, description, kind, status, window, compare-at mode on `PriceList`.
   - Compare-at on variants and fixed prices.
   - `ResolvedPrice` through carts, offers and orders.
   - The migrations (naming existing lists), routes and DTOs.
2. **Many lists per catalog and lowest wins:**
   - `catalog_price_lists`, the catalog routes.
   - Scoped adjustments and the category read in `load_pricing`.
   - The `prices` read route.
3. **Breaks:** on fixed prices, in the cart and in offers.
4. **Schedules and search:**
   - The `price_list_schedule` step.
   - The started/ended events and Workers marks.
   - `compare_at` / `on_sale` in search documents, the contract, the facet.
5. **Group audience:** `price_list_groups`, the buyer's groups in the pricing context,
   `group_in_use`, the storefront's session-priced product read.
6. **The admin:** the list pages, templates, grid, CSV, the product page's prices card.
7. **The storefront:** strikethroughs, breaks, the sale shelf and filter, signed-in
   group prices. The seed gains a sale, a clearance list and a wholesale group.

## As built

Where steps 4 and 5 differ from the sections above:

- **The schedule's progress** (§7) is `price_list_schedule.ran_until`, moved in the same
  commit as the events it announced (`StagePriceListSchedule`; a run that lost the race
  gets `StaleVersion` and announces nothing). The first run ever only notes where it
  starts. A run that finds nothing to announce writes nothing, so a quiet store's job
  costs two reads a minute.
- **What's announced**: the start and end of every *active* list that passed since then,
  oldest first, unless the list was written at or after that moment (its own write
  recorded `price_list.changed`). A draft made active inside its window records
  `price_list.changed {fields: ["status"]}` and no `started`. Payloads are
  `{price_list, catalogs, starts_at}` and `{price_list, catalogs, ends_at}`, the times in
  microseconds since the epoch like `customer.erasure_requested`'s `due_at`.
- **Search** rebuilds everything on either event when `catalogs` isn't empty (as on
  `price_list.changed`), rather than working out the affected products. The storefront
  expires its `prices` tag on them, and now on `catalog.price_lists_added` / `_removed`.
- **Workers**: the step runs before dispatch on the ticks the marks wake, and after every
  write; its `retry_at` is the next window boundary, so the `due` mark wakes a tick for
  it. Natively it's `[jobs.price_list_schedule]` (every minute), waking the dispatcher
  when it announced something.
- **`on_sale` is a facet key**, `on_sale` with values `true` / `false`, so the one
  `filter=on_sale:true` both filters and (added to the merchant's facets) counts; it
  isn't a default facet. Documents carry `compare_at` and `on_sale`, hits answer both.
  A search that filters or counts `on_sale` is always the built-in engine's: a search
  app indexes from webhooks and can't know what's on sale. Migration 0079 empties the
  index (the host rebuilds it on start), as 0039 did.
- **The audience** is `{"groups": [...]}` on create, `PATCH` (replacing it; `[]` for
  everyone) and reads, sorted, at most 100, each once (`too_many_audience_groups`,
  `duplicate_audience_group`). Markets can't see groups, so storage checks they exist
  (`price_list_groups`' reference: `422 customer_group_not_found`) and refuses deleting
  one an audience names as `409 customer_group_in_use` (not `group_in_use`: customers'
  codes all start `customer_group_`).
- **The buyer's groups** reach pricing as plain IDs: `PricingContext.groups`, passed to
  `load_pricing` / `load_pricing_for`, which load fixed prices only for lists live for
  them. `hypha-cart` reads them (`buyer_groups`, from the cart's customer, or
  `Ctx::customer` for a product read sent with a customer session); no new crate edge.
  Search documents and reads without a session pass none. A list outside the buyer's
  audience is the candidate result `not_in_audience`; `GET /admin/products/{id}/prices`
  takes `group` to price for a customer in it (`404 customer_group_not_found`), and
  `GET /admin/price-lists` takes `group` too.
