# 0021: Store setup

- **Status:** Accepted (2026-10-16; section 11 answered). After sales channels (0020), Danilo set the next
  step: the first storefront, with the admin pieces it needs built alongside. A survey
  of what's missing found that most of it is API and data model, not screens. His
  answers so far:
  - **Payments:** none in version 1. Orders stay unpaid (cash on delivery in
    practice). A real cash-on-delivery method is a spore, and the spore API comes later.
  - **Categories:** plain parent/child categories, not Shopify's collections. That's
    note 0022.
  - **Languages:** English only for now, with room left for more (section 5).
  - **URLs:** one folder per country in our storefront. The API mustn't force it on
    anyone building their own storefront (section 6).
  - **Order:** store setup and admin first, then the storefront.
- **Scope:**
  - every ISO currency and checked country codes;
  - the admin reads that setup screens need (markets, catalogs, price lists, tax);
  - store details;
  - room for languages;
  - the storefront's routing contract;
  - the storefront reads a product page and `proxy.ts` need;
  - the admin screens: Markets, store settings, the product editor's save bar, a setup
    checklist;
  - an outline of the storefront app.
- **Not in scope:**
  - payments and the spore API;
  - categories (0022);
  - translations themselves;
  - customer accounts (0008 §9);
  - a domain per market;
  - the storefront app's own design note (it follows this one).
- **Related:** 0010 (API surfaces), 0011 (the admin), 0012 (lists), 0013/0014 (media),
  0019 (search), 0020 (channels), 0022 (categories).

## Decisions

| | Decision |
| --- | --- |
| SU1 | **Every ISO 4217 currency a shop can charge in.** `Currency` becomes a validated code backed by a table in core: the code and its minor units (JPY 0, EUR 2, KWD 3). It stays `Copy`, and the JSON is the same string. Funds, metals and test codes (XDR, XAU, XXX) are left out. *Hard to reverse* |
| SU2 | **Country codes are checked against ISO 3166-1 alpha-2.** Today any two capital letters pass. The same table in core gives each country's English name, for agents and the CLI; UIs show local names with `Intl.DisplayNames`. *Hard to reverse* |
| SU3 | **The admin can read its whole setup.** List and fetch routes for markets, catalogs (with their products, paged), price lists (with their fixed prices, paged), tax classes and tax regions. Each store has only a handful of these, so the top lists come unpaged, as channels do. Plus `GET /admin/currencies` and `GET /admin/countries` for pickers. *Decided* |
| SU4 | **Store details.** Besides the name, all optional: contact email, a short plain-text description, legal name and address, timezone (checked against IANA names, Q4), and the default language (SU5). No logo: branding (logos, light and dark, a wordmark) waits for a content system (Q3). The storefront reads the public part. *Decided* |
| SU5 | **Room for languages, without building them.** The store gets a `default_language` (BCP 47, `en` to start): the language its content is written in. Translations will later be overlays (resource, field, language) in a crate of their own, and storefront reads will take a `language` with fallback to the default. Nothing about products or categories changes now. *Decided* |
| SU6 | **The API is indifferent to URL shape.** Every storefront read that prices or lists takes `country` (search already does), and a cart takes it when created. Our storefront uses one folder per country by default, with a setting for one shared tree where the country comes from a cookie. `GET /storefront/countries` lists where the store sells, with each country's market and currency. *Decided* |
| SU7 | **A product page in one read:** `GET /storefront/products/by-handle/{handle}?country=…` (the read by ID gets `country` too). It returns the product, its media, options, and each variant's price in that market and whether it's available. Drafts and unpublished products stay 404. *Decided* |
| SU8 | **No payment step in version 1.** Checkout places the order unpaid, as it does today; staff mark it paid (`POST /admin/orders/{id}/paid`). Payment methods come with the spore API. *Decided* |
| SU9 | **A setup checklist on the admin's home**, worked out from data rather than stored: store details, a market, a location with stock, an active product, published to a channel, and a storefront key. *Decided* |

## 1. Currencies (SU1)

- `hypha_core::Currency` becomes `Currency([u8; 3])`. `Currency::new("JPY")` looks the
  code up in a static table (code → minor units) and fails with
  `CurrencyError::Unknown`. `Currency::minor_units()` returns 0, 2, 3 or 4. The type
  stays `Copy`, and no I/O is involved, so it still builds for wasm.
- Amounts stay integers in minor units, unchanged. The minor units only tell a client
  where the decimal point goes. `Money::MAX_AMOUNT` and the rounding rules don't
  depend on it.
- **What changes for clients:** in OpenAPI, `Currency` turns from a three-value enum
  into a string with a pattern and examples. The SDK's type widens from
  `"EUR" | "USD" | "RSD"` to `string`. The JSON itself doesn't change.
- **Storage:** currencies are already text columns. A migration checks that every
  stored code is in the table. Today it can only be EUR, USD or RSD, so this is a
  safeguard, not a fix.
- **The admin** gets minor units from `GET /admin/currencies` rather than assuming two.
  `parseMoney` and `formatMoney` take them into account (`Intl.NumberFormat` already
  agrees for display).
- **Where the table comes from:** ISO 4217's published list, frozen as a Rust array.
  Updating it is a code change, which is fine since it changes a few times a decade.

## 2. Countries (SU2)

- `CountryCode::new` also checks the code is in core's ISO 3166-1 table; a code that
  isn't fails with `CountryError::Unknown`. The table holds about 250 assigned codes
  with English names.
- `restore` re-runs the check. The migration first lists any stored country that isn't
  in the table (markets, shipping zones, tax regions, addresses, carts, orders). If
  there are any, it fails with their values, so nothing silently stops loading.
- `GET /admin/countries` returns `[{code, name}]`. Selects show
  `Intl.DisplayNames(locale)` names and fall back to the English one.

## 3. The admin's reads (SU3)

| Route | Returns |
| --- | --- |
| `GET /admin/markets`, `GET /admin/markets/{id}` | markets: name, countries, currency, prices include tax; plus each market's catalog ID |
| `GET /admin/catalogs/{id}` | the catalog: market, price list |
| `GET /admin/catalogs/{id}/products` | its products, paged (0012) |
| `GET /admin/price-lists`, `GET /admin/price-lists/{id}` | price lists: currency, adjustment |
| `GET /admin/price-lists/{id}/fixed-prices` | fixed prices, paged |
| `GET /admin/tax-classes`, `GET /admin/tax-regions` | everything (few) |
| `GET /admin/currencies`, `GET /admin/countries` | reference lists for pickers |

Each repository gains `list` where it lacks one; those methods get contract scenarios
first.

## 4. Store details (SU4)

`Store` gains these fields:
- `contact_email: Option<Email>`;
- `description: Option<String>` (plain text, at most 500 characters);
- `legal_name: Option<String>`;
- `legal_address: Option<Address>`;
- `timezone: Option<Timezone>`;
- `default_language: Language`.

**Timezone** needs care: it's checked against the IANA names, and the full database is
heavy for wasm, so Q4 asks how strict to be.

`GET /storefront/store` gains the public part: name, description, contact email and
default language.

## 5. Room for languages (SU5)

- `Language`: a validated BCP 47 tag (`en`, `en-GB`, `sr-Latn`), in core.
- The store's `default_language` says which language every content field (titles,
  descriptions, option labels, category names) is written in. It's `en` until a
  merchant changes it.
- **Later:** translations in their own crate. Rows of resource, field, language and
  text. Markets list the languages their storefront offers, and storefront reads take
  `language`, falling back to the default. Handles stay one per resource until then.
  Translated URLs are their own decision.
- **What this rules out now:** nothing stores a language next to content, and content
  isn't keyed by language. Translations are added beside it, never by reshaping it.

## 6. Storefront routing (SU6)

- **The API's contract:** a storefront names the shopper's country on reads that
  price, list or search, and when it creates a cart. The cart then keeps it. Nothing
  in the API knows about URLs.
- **`GET /storefront/countries`** lists every country a market sells to, with its
  market's currency, minor units and whether prices include tax. That's what
  `proxy.ts` and a country picker need.
- **Our storefront:**
  - **Folders (the default).** `proxy.ts` sends `/` to `/{country}/` from a cookie,
    else the geolocation header the host gives (Vercel's, Cloudflare's), else the
    store's first country.
  - **One tree.** A setting turns the folders off: URLs carry no country, the country
    lives in a cookie, and a picker changes it.
  - **A storefront someone builds themselves** picks either, or neither.

## 7. The storefront's product read (SU7)

```
GET /storefront/products/by-handle/{handle}?country=HR
```

- **Returns** the product's content and media, its options, and its variants. Each
  variant carries its price in the market's currency, whether it's available
  (`available`, from stock), its options and its media.
- **404 `product_not_found`** for a draft or archived product, or one the key's channel
  doesn't sell (CH6).
- **A product the country's market doesn't sell** is Q2: either 404, or the product
  with `sold_in_country: false` and no prices.
- **Pricing** reuses the cart's loaders (`load_pricing`, `StockContext::load`), so the
  product page, search and the cart always agree.
- `GET /storefront/products/{id}` takes the same `country`. Without it, it returns no
  prices, as today.

## 8. Admin screens

- **Settings → Markets:**
  - list markets with their countries and currency;
  - a market page: name, countries (a multi-select with flags and local names),
    currency (a searchable select), prices include tax;
  - its catalog: products in it, add and remove;
  - its price list: adjustment and fixed prices.
- **Settings → Store:** the SU4 fields.
- **Taxes** (Settings → Taxes): tax classes and regions. They can wait until the
  storefront needs more than the checklist.
- **The product editor's save bar:** editing a product no longer saves card by card. One
  draft of the product covers details, content, options and variants, and a bar at the
  top says "Unsaved changes" with Discard and Save. Save sends the product's PATCH, then
  `PUT /admin/products/{id}/variants` (Q1). The options card offers "Create all
  variants", which fills the draft with the combinations for the merchant to price and
  save.
  Media uploads and publishing still act at once: an upload can't wait for Save, and
  publishing is a separate decision.
- **Home's setup checklist (SU9):** each step links to its screen and is ticked when
  the data shows it done.

## 9. The storefront app (outline; its own note follows)

`apps/storefront`, the admin's stack (0011):
- Next.js App Router on vinext, server-rendered.
- The publishable key stays on its server.
- `@hypha/sdk`'s storefront client.

The pages:
- **Home:** the store's name, top categories (0022), and new products (search sorted by
  newest).
- **Category:** the category's products through search, filtered by it (0022),
  with facets and paging.
- **Product:** SU7. Option pickers choose the variant, then add to cart.
- **Search:** the existing `/storefront/search`.
- **Cart and checkout:**
  - email and shipping address;
  - shipping options (0015);
  - place the order (unpaid, SU8);
  - the order confirmation, read through the cart (`view_cart`).
- **Country picker and `proxy.ts`:** section 6.

## 10. Build order

Each step ends green (fmt, clippy, tests, wasm, the Postgres contract, e2e).

1. Core: `Currency` from the ISO table; `CountryCode` checked; `Language`. Migrations
   check the stored codes.
2. Admin reads (SU3) with their contract scenarios; `GET /admin/currencies` and
   `GET /admin/countries`.
3. Store details (SU4) and their storefront part.
4. Storefront reads: `GET /storefront/countries`; the product read with prices (SU7).
5. Admin: Settings → Markets, Settings → Store, the setup checklist.
6. `PUT /admin/products/{id}/variants` (Q1), then the admin's product editor: the save
   bar and "Create all variants".
7. Categories (0022), in parallel with 6.
8. The storefront app, with its own note.

## 11. Open questions

**Q1. How the product editor's Save reaches the API.** *Decided: a bulk variants route.*
- `PUT /admin/products/{id}/variants` sets a product's whole variant list in one
  batch. A variant in the list with an ID is updated; one without an ID is added; a
  stored one that's missing is removed. All or nothing, with one set of events.
- Save is then at most two calls: the product's PATCH (details, content, options), then
  this.
- **"Create all variants"** (Danilo: wanted when options are set up) uses the same
  route. The admin builds the combinations of the chosen option values itself and
  sends them as an explicit list. The API never expands options into variants
  (CLAUDE.md: sparse explicit variants), and `Product::MAX_VARIANTS` (2,048) bounds
  the list, so a combination that's too large is refused, not cut short.
- A single "save product" command (one call for everything) can come later if two calls
  prove a problem.

**Q2. A product the shopper's country doesn't sell.** *Decided: 200 with
`sold_in_country: false`,* and no prices or availability. A shopper who followed a link
from another country sees "not available in your country", not a missing page. Search
already leaves such products out of listings.

**Q3. The store logo.** *Decided: not in this note.* Danilo: one logo leads straight to
light and dark logos and a wordmark. Branding belongs with a later content (CMS-style)
system, so it's designed once there.

**Q4. Timezones.** *Decided: checked against a compiled list of IANA names* (about 600
strings, no rules data), in core like the currency and country tables. Conversions happen
in the clients, which have the full database. The list is refreshed by a code change,
like the others.
