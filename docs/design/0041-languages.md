# 0041: Languages

- **Status:** Accepted (2026-11-13). Danilo read ML1–ML16 and agreed with every
  recommendation in §14:
  - Serbian (`sr-Latn`) is the second UI language; German is the demo store's second
    content language (Q1).
  - URLs are `/{country}` and `/{language}-{country}` (Q2).
  - Orders keep their snapshots in both languages (Q3).
  - Our own thin layer on `intl-messageformat`, not next-intl (Q4).
  - Clients translate built-in messages by `code` (Q5).
  - A generated CLDR table for email formats (Q6).
  - Custom field definitions first, values last (Q7).
  - §17's order, with the admin's string extraction on its own branches (Q8).
  - §15's hard-to-reverse choices and §16's two edges are accepted with them.
- **Scope:**
  - Translating the merchant's content (products, categories, custom fields, shipping
    and payment names, discounts, checkout rule messages) and serving it to shoppers.
  - Which languages a store has and which each market offers.
  - A language on carts, orders and customers, and emails in the buyer's language.
  - The storefront in several languages: URLs, `hreflang`, UI strings, formats.
  - The admin in several languages: UI strings and formats, per staff member.
  - The admin's screens for translating content.
  - Search in each language.
- **Not in scope:**
  - Translated handles (one URL slug per resource stays; §13).
  - Translating `vendor`, `product_type` and tags (§13).
  - Machine translation built in (agents can already do it through MCP, §10; an
    extension point for translation apps is §13's).
  - Right-to-left layouts beyond setting `dir` (§13).
  - `apps/site` and the docs.
  - Language-specific stemming and CJK word splitting in search (§13).
- **Related:**
  - 0021 SU5 and §5 (the room left: `Language`, `default_language`, "translations are
    added beside content, never by reshaping it").
  - 0019 (search: "documents per language when translations exist").
  - 0022 (category name and description marked translatable), 0033 (custom field
    labels and values through overlays).
  - 0028 Q2 and Q3 (templates keyed `(key, locale)`, locale-aware formats deferred).
  - 0040 §13 (translated messages, labels and region names deferred to this note).
  - 0023 (storefront routing by country), 0035 (agents).

## Why

Hypha sells to several markets with their own currencies and prices, in one language.

1. **Content has one value.** A product has one title and one description. A store
   selling in Germany and France shows both the same text.
2. **Nothing knows the shopper's language.** Carts, orders and customers store none.
   `Ctx` has none. Order emails are English for everyone.
3. **The storefront is English in code.** About 40 of its 57 component files hold
   literal English. Money is formatted with `"en"`, dates with `"en-GB"`. Its
   `hreflang` is `<default language>-<COUNTRY>` for every country.
4. **The admin is English in code.** About 196 of 238 component files hold literal
   English, and `lib/hypha.ts` maps about 185 error codes to English sentences. A
   merchant's staff in Belgrade or Berlin can't work in their own language.
5. **The API writes English for shoppers** in a few places: built-in cart messages,
   address form labels, discount summaries, default email templates.

What's already in place: `Language` (a checked BCP 47 tag) in core,
`Store::default_language`, notification templates keyed `(key, locale)`, IDs on product
options and their values, and Astryx's own catalogs in 31 languages.

## Decisions

| | Decision |
| --- | --- |
| ML1 | **Four kinds of text, four mechanisms.** The merchant's content: overlays stored by the API (§3). Text the API writes: clients translate by `code`, the API renders only what nobody else can, emails (§7). The apps' own UI strings: catalogs in each app (§8). Formats (money, dates, numbers): the request's locale everywhere (§8) |
| ML2 | **The store's `default_language` stays the source language.** Content fields keep holding it. The store gains a list of other languages, each published or not; a market offers a subset in order, its first being its default (§2) |
| ML3 | **A new leaf, `hypha-translations`:** one `Translation` per (resource, language), holding the translated fields. Aggregates don't change. A closed catalog says which resources and fields are translatable and each field's rule (§3) |
| ML4 | **Regional wording is a language of its own:** `en-GB` beside `en`, `pt-BR` beside `pt`. A read falls back from `de-AT` to `de` to the source. No separate "per market" translations as Shopify has |
| ML5 | **Storefront reads take `language`,** a query parameter like `country`. The API answers the same DTOs with translated values in place and a `Content-Language` header. Admin reads always answer the source; translations have their own routes (§4) |
| ML6 | **Translations are applied where a response or a snapshot is built,** not inside leaves: `hypha-http` for product, category and cart responses; `hypha-checkout` for what an order copies; `hypha-search` for documents. Two new edges (§16) |
| ML7 | **Carts, orders and customers get a `language`.** The order keeps its lines' titles in both the source language (for staff) and the buyer's (for emails and the confirmation page) (§5) |
| ML8 | **A translation remembers what it translated:** each field keeps a digest of the source text. The admin shows a translation as outdated when the source's digest differs. Nothing is deleted or hidden when the source changes (§3.3) |
| ML9 | **Handles aren't translated.** One URL slug per resource; languages differ by the URL's prefix (§9, §13) |
| ML10 | **Search: one document per variant, market and language** the market offers. Facet values keep the source text as their key and carry a label in the document's language, so a filter's URL means the same in every language (§6) |
| ML11 | **The API's `error` strings stay English.** `code` and `details` are the contract; each app maps the codes it can meet. Built-in cart messages gain `params` so a client can render them itself; `message` stays as the English fallback (§7) |
| ML12 | **Emails are rendered in the order's language:** the merchant's template for that language, else the build's default for it, else the default language's. Money and dates in emails follow the language (§7.3) |
| ML13 | **App strings are ICU MessageFormat catalogs** (`messages/<language>.json` per app), read through a small shared package on `intl-messageformat`. The same format Astryx ships its own catalogs in. No framework plugin (§8) |
| ML14 | **The admin's language belongs to the staff member** (`users.language`), then the browser's, then English. It has nothing to do with the store's content languages (§8.3) |
| ML15 | **Storefront URLs: `/{country}` is the market's default language, `/{language}-{country}` another** (`/de`, `/en-de`). Existing URLs keep working (§9) |
| ML16 | **Scopes `translations:read` and `translations:write`.** Agents may hold both, so an agent can translate a catalogue through MCP, as proposals where the grant says so (§10) |

## 1. What others do

**Shopify** ([`translatableResource`](https://shopify.dev/docs/api/admin-graphql/latest/queries/translatableResource), [`translationsRegister`](https://shopify.dev/docs/api/admin-graphql/latest/mutations/translationsRegister))
- Overlays beside the content: (resource, key, locale, value), with an optional
  `marketId` for wording that differs in one market.
- Each translatable field has a digest; a translation is registered against it and is
  flagged `outdated` once the source changes.
- Languages are published per market; URLs are `/en-ca`. Handles are translatable.
- Storefront reads take the language in context (`@inContext(language:)`).

**Medusa v2** ([Translation Module](https://docs.medusajs.com/resources/commerce-modules/translation), [storefront](https://docs.medusajs.com/resources/commerce-modules/translation/storefront), new in 2.12–2.13)
- A module of its own: rows of `reference`, `reference_id`, `locale_code` and a
  `translations` object keyed by field.
- Store routes take `locale` (query) or `x-medusa-locale` (header); a missing
  translation falls back to the original text.
- Carts and orders have a locale; line items are re-translated when it changes.
- Translatable: product title, description; variant, category, collection, option and
  option value, tag and type; shipping option, region, tax rate names.

**Vendure** ([languages](https://docs.vendure.io/current/core/core-concepts/language))
- Every translatable entity owns a `translations` array (a row per language); there is
  no "source" text outside it.
- `?languageCode=de` per request; each channel has `availableLanguageCodes` and a
  `defaultLanguageCode`; fallback to the default.
- The search index has a row per variant, channel and language.

**Saleor** ([translations](https://docs.saleor.io/api-usage/i18n))
- A translation table per model, read as `translation(languageCode:)` beside the
  original fields, so the client picks which to show.

**What we take:**
- Overlays with a source digest (Shopify), in their own crate (Medusa), as 0021 planned.
- One `language` parameter with fallback done by the API (Medusa, Vendure), not left to
  the client (Saleor).
- Languages per market (Shopify, Vendure). Documents per language (Vendure).

**What we leave:**
- Vendure's shape (content inside translations) would reshape every aggregate.
- Shopify's market-specific translations: a regional tag does the same with one
  mechanism (ML4).

## 2. Languages (ML2)

- **Store:** `default_language` (as now) and `languages: [{language, published}]`.
  - An unpublished language can be translated and previewed but no storefront read
    serves it.
  - At most 20 besides the default.
  - The default language can change only while the store has no translations
    (`409 default_language_in_use`): every overlay is "a translation from it".
- **Market:** `languages: [Language]`, ordered, each one the store's default or a
  published language. The first is the market's default. Empty means the store's
  default alone, which is every market today.
- `GET /storefront/countries` gains each country's `languages` (the market's, in
  order), so a storefront can route and build its switcher from one read.
- The admin's "Content language" free-text input becomes a select, and Settings gets a
  Languages page (add, publish, unpublish, remove with its translations).

## 3. Translations (ML3, ML8)

### 3.1 The aggregate

A `Translation` is identified by `(resource, language)`, like a notification template
by `(key, locale)`; no ID of its own.

- `resource`: a `ResourceRef { kind, id }`. `kind` is a closed enum; `id` the resource's
  UUID.
- `language`: never the store's default.
- `fields`: a map from field key to `{ value, source_digest }`.
- Versioned and stamped like every root. One write per resource and language, which is
  what the admin's editor saves.

A resource's children live in its translation, addressed by their IDs:

| Kind | Field keys |
| --- | --- |
| `product` | `title`, `description`, `options.<opt_…>.name`, `option_values.<optval_…>.label` |
| `category` | `name`, `description` |
| `custom_field_definition` | `name`, `description`, `choices.<key>.label` |
| `custom_field_value` (owner + definition) | `value` for `text`, `multiline_text`, `rich_text` |
| `shipping_option` | `name` |
| `payment_method` | `name`, `instructions` |
| `discount` | `title` |
| `checkout_rule` | `message` |
| `asset` | `alt` |
| `store` | `description`, the email footer |
| `search_facet` | `label` |

Variant titles aren't stored: they're joined from option value labels, so they come out
translated.

### 3.2 Rules

- The catalog gives each field a rule: plain text up to N characters, or HTML
  (sanitised like `Description`, same size cap). A translated value obeys the rule of
  the field it replaces. A host test holds the catalog's limits to the domains'.
- The leaf can't see products or categories. The command takes what it needs as plain
  values from the host: that the resource exists, and each source field's current
  digest.
- A key for a child that no longer exists (a removed option value) is refused on write
  and ignored on read.
- An empty value removes the field: the source shows again.

### 3.3 Outdated

- `source_digest` is SHA-256 of the source text at the time of translating.
- The admin's translation routes answer each field with the source text, its digest
  now, the translation and `outdated: bool`.
- Storefronts keep getting the outdated translation. A wrong word in German is better
  than a sudden English paragraph.

### 3.4 Storage, events, cleanup

- One table: `translations (resource_kind, resource_id, language, fields jsonb,
  version, created_at, updated_at)`, primary key on the first three. Loading the
  translations of a page of products in one language is one query.
- Events `translation.updated` (resource, language, field keys) and
  `translation.deleted`, routed to apps with `translations:read`.
- A `TranslationCleanup` handler removes a deleted resource's translations (there's no
  foreign key from a generic table).
- The storefront's webhook subscribes to both and expires the resource's cache tags.

## 4. Reading (ML5, ML6)

- **Parameter:** `language=<tag>` on storefront reads. On carts it's a stored field
  instead (§5).
- **Resolution,** in the API:
  1. No `language`: the market's default (so today's clients see no change).
  2. A language the market doesn't offer or the store hasn't published: the market's
     default. Never an error, so a stale link still renders.
  3. Per field: the exact tag, then the tag without its region, then the source.
- **Answer:** the same DTOs. `Content-Language` names the language resolved in step 1
  or 2 (not whether each field had a translation).
- **Where it's applied:** the host loads the overlays for the resources in a response
  (`Overlay::load(ports, language, &[ResourceRef])`) in the same round trip as the
  resources, and DTO builders ask `overlay.text(resource, key)` before the source.
- **Admin surface:** reads answer the source language, always. Translations are read
  and written through:
  - `GET /admin/translations/{kind}/{id}`: every field's source, digest and each
    language's value.
  - `PUT /admin/translations/{kind}/{id}/{language}`: the fields of one language.
  - `DELETE` the same.
  - `GET /admin/translations?language=&kind=&state=missing|outdated|complete`: what's
    left to translate, paged.
- **Webhooks and extension requests** carry source text. Requests about a cart
  (`checkout.validate`, `payments.start`, `shipping.rates`) gain the cart's `language`.

## 5. Carts, orders, customers (ML7)

- **Cart:** `language`, set at creation and changeable (`PUT …/language`), held to what
  the cart's market offers. Cart responses are in it: line titles, shipping option
  names, payment method names, rule messages.
- **Order:** `language` copied from the cart. Its snapshots (line `product_title` and
  `variant_title`, shipping option name, payment method name and instructions, discount
  titles) stay in the source language, and each gains a `localized` copy in the order's
  language when that differs.
  - Staff read orders in the store's language. The buyer's emails and confirmation
    page read `localized`.
  - Old orders have no language and no `localized`: they read as the default.
- **Customer:** `language`, set from their latest cart or their account profile; used
  for emails that aren't about one order (the login code).
- **Admin:** an order shows its language; the customer page too.

## 6. Search (ML10)

- A document per variant, market and language the market offers; the key gains
  `language`. A market with one language has the documents it has now.
- Text terms come from the translated title, description and option labels (the
  tokenizer has no stemming, so it works for any language written with spaces).
- **Facets:** a value's key stays the source text (`option.color` = `Red`), and the
  document carries its label in its language (`Rot`). The storefront shows labels and
  puts keys in the URL. Switching language keeps the filters, and `hreflang` pairs
  point at the same listing.
- `SearchIndexer` wants `translation.updated` / `deleted` and the store's and markets'
  language changes; it marks the products concerned, as now.
- `search.query` (the search app's point) gains `language`.

## 7. Text the API writes (ML11, ML12)

### 7.1 Errors

- `error` is for developers and stays English. Each app already maps codes to its own
  sentences (`describe()` in both apps); those sentences move into the catalogs.
- The storefront's `describe()` falls back to `error.error` for an unknown code. In
  another language it falls back to a generic sentence instead.

### 7.2 Cart messages and address forms

- A rule's message is content: translated through `checkout_rule` overlays.
- A validation app's message is the app's: it's told the language.
- Built-in messages (format and verification) gain `params` (`{field, example,
  region}`), enough for a client to build the sentence from `code`. `message` stays, in
  English, for clients that don't.
- Address forms answer label **keys** beside the English labels (`postal_code:
  "zip_code"`, `province: "prefecture"`): a closed set of about 40, translated in the
  storefront's catalog.
- Region names stay as generated (the country's own names, mostly). Localised region
  names are in `worldwide`; taking them is §13's.

### 7.3 Emails

- **Template lookup** for an order in `de-AT`: the merchant's `(key, de-AT)`, their
  `(key, de)`, the build's default for `de`, then the default-language chain as today.
- **Defaults per language:** `packages/emails` builds its React Email templates into
  Jinja once per catalog language, from the same ICU catalogs as the apps.
- **Context:** line titles and names come from the order's `localized` copies.
  `order.language` and `dir` are added. Variable names don't change.
- **Formats:** `format_money` and `TimeValue` take the language. The data is a
  generated table per language (decimal and grouping marks, where the currency sign
  goes, month names, date order), built from CLDR by a script like the address data.
  Not ICU4X: its data for dates and numbers adds megabytes to the Worker (Q6).
- **Discount summaries** are an English sentence built in the leaf. Emails in another
  language show the discount's translated title instead. Both apps already can (the
  admin does) build the sentence from the typed data.
- The admin's template editor gets a language switch; preview and test send take a
  language.

## 8. The apps' own strings (ML13, ML14)

### 8.1 Mechanism

- `packages/i18n` (`@hyphacommerce/i18n`): `createTranslator(language, catalog)` on
  `intl-messageformat`; a provider and `useT()` for client components; `getT()` for
  server components and actions. Web APIs only.
- Catalogs: `apps/<app>/messages/en.json`, ICU MessageFormat, flat dotted keys
  (`cart.items_count`: `{count, plural, one {# item} other {# items}}`).
- Keys are typed from `en.json`, so a missing key doesn't compile. A check fails when a
  catalog has a key `en.json` lacks, and reports what each still misses.
- A lint rule refuses literal text in JSX once an app is extracted.
- A pseudo-language in dev shows what was missed.
- Astryx's `InternationalizationProvider` gets the same language and its own shipped
  catalog; `<html lang dir>` is set by the root layout.

**Why not next-intl:** its server side is wired through a Next config plugin that
aliases a module with webpack or Turbopack, and vinext has neither. It might work with
a Vite alias, but that's a vinext-specific path, which the apps avoid. The layer above
is about 150 lines and has no framework in it (Q4).

### 8.2 Formats

- One `locale` per request: `<language>-<COUNTRY>` on the storefront (`de-CH` formats
  CHF the Swiss way), the staff member's language in the admin.
- Every `Intl.*` and `toLocaleString` call takes it (about 45 hard-coded `"en…"` today).
  `formatMoney`, the date components and `Intl.DisplayNames` (country, currency and
  language names) go through helpers that do.

### 8.3 The admin's language

- `users.language: Option<Language>`, set in the profile (`PATCH /admin/me`).
- Resolution: the user's, then `Accept-Language` matched against the catalogs, then
  `en`. The login pages use the last two.
- The admin shows content in the store's default language whatever the UI language is.

### 8.4 Which languages ship

- English is the source. Each further language is a catalog file per app plus the
  email defaults: a contribution anyone can make, checked by CI for completeness.
- Shipping the first second language with the feature proves the path (Q1).

## 9. The storefront (ML15)

- **URLs:** `/{country}/…` in the market's default language; `/{language}-{country}/…`
  in another (`/en-de/…`, `/sr-latn-rs/…`: the last part is the country). One dynamic
  segment as now, parsed by `lib/site.ts`. In `single` routing: no prefix for the
  default language, `/{language}/…` for another.
- **Choosing:** the proxy picks a language only when it redirects a bare path: the
  `language` cookie, then `Accept-Language` against the market's languages, then the
  market's default. A URL that names a language is never redirected by headers.
- **Switcher:** in the header beside currencies, shown when the market has more than
  one language; it keeps the page and the listing's filters.
- **Static pages:** the language is in the path, so pages stay static with ISR. Cached
  reads add the language to their keys (`["product", handle, country, language]`;
  `store`, `categories` and `custom-field-definitions` too, which have no dimension
  today). Tags stay per resource.
- **SEO:** `hreflang` for every (language, country) pair as `de-CH`, `x-default` as
  now; canonical is the page's own URL; `og:locale`; JSON-LD `inLanguage`; the sitemap
  lists every pair.
- **Cart:** created with the page's language; switching language updates it.
- **Order links in emails** carry no language: the route handler redirects to the
  order's.

## 10. Translating in the admin (ML16)

- **A "Translate" action** on product, category, discount, shipping option, payment
  method, custom field and checkout rule pages opens the editor: a language select,
  the source on the left, the translation on the right, an "Outdated" badge with the
  source's change, the rich text editor for HTML fields. One save per language.
- **Settings → Languages:** the list, publish, each language's progress (translated,
  outdated, missing), and a link into the worklist (`GET /admin/translations?state=`).
- **Markets:** a languages field on the market page.
- **Agents:** the translation routes are operations like any other. An agent with
  `products:read` and `translations:write` can work through the missing list; with a
  propose-only grant each write waits for a person. This is the machine translation of
  v1, and it needs nothing built.
- **Scopes:** reading a resource's translations needs `translations:read` and the
  resource's own read scope; writing, `translations:write` and the same read scope.

## 11. Privacy and personal data

- Translations hold no personal data: customer and order custom field values aren't
  translatable (the `custom_field_value` kind covers product, variant and category
  owners only).
- A customer's and an order's `language` aren't redacted.

## 12. API, events and migrations (summary)

- Storefront: `language` on product, category, search, store, custom field definition,
  payment method and address format reads; `language` on cart create and
  `PUT /storefront/carts/{id}/language`; `languages` on countries.
- Admin: the translation routes (§4), store and market `languages`, `users.language`,
  `language` on orders and customers.
- Events: `translation.updated`, `translation.deleted`; `store.updated` and
  `market.updated` gain `languages` among their fields; `order.placed` gains
  `language`.
- Migrations: `translations`; `stores.languages`; `markets.languages`;
  `carts.language`, `orders.language` and the `localized` snapshot columns;
  `customers.language`; `users.language`; the search documents' key.

## 13. Later

- **Translated handles,** with redirects from the other languages' slugs.
- **`vendor`, `product_type`, tags.** They're filter values as well as text; they need
  the facet key/label split first.
- **An extension point for translation apps** (`translations.translate`: DeepL, a
  model), and an "Auto-translate" button on it.
- **CSV or XLIFF export and import** for agencies.
- **Localised region and country names from the API** (worldwide and CLDR have them).
- **Right-to-left:** an audit of both apps' CSS for logical properties. Astryx's
  components already follow `dir`.
- **Search:** stemming per language, CJK segmentation.
- **API-side catalogs** for built-in messages, if UCP agents need sentences rather
  than codes.
- **Translated email subjects per market,** store names per language.

## 14. Questions

**Q1. Which second language ships first?** It decides who can review the catalogs.
Recommended: Serbian (`sr-Latn`), since you can judge it, with German as the demo
store's second market language (content only, translated by an agent through MCP as a
demonstration).

**Q2. URL shape (ML15).** `/{language}-{country}` (`/en-de`), or two segments
(`/de/en/…`)? Recommended: the first. It's one segment as today, it's what `hreflang`
looks like, and Shopify stores have taught people to read it.

**Q3. Orders keep both languages (ML7)?** The alternative is one copy in the buyer's
language (Medusa), which leaves staff reading German line titles. Recommended: both.
It costs a second set of snapshot columns.

**Q4. Our own thin layer or next-intl (ML13)?** Recommended: our own, for the reason in
§8.1. If you'd rather have next-intl's ecosystem, the first step is a spike to see
whether it runs under vinext without a vinext-only alias.

**Q5. Built-in messages: clients translate by code (ML11), or the API holds catalogs?**
Recommended: clients, now. The API catalogs can be added later without breaking
anything, since `message` stays.

**Q6. Email formats: a generated CLDR table or ICU4X (§7.3)?** Recommended: the table.
It covers money and dates in emails, which is all the API formats.

**Q7. Is custom field value translation in the first build?** It's the largest single
piece of §3 (values are keyed by owner and definition, lists included). Recommended:
definitions and choice labels first, values as the last step.

**Q8. Order of work.** The content side (§2–§6, §9) is what merchants are waiting for;
extracting the admin's strings (§8) is large and mechanical and touches almost every
file. Recommended: build in §17's order, with the admin's extraction on its own branch
so it doesn't block or conflict with the rest.

## 15. Hard to reverse

- **The overlay's shape:** kinds, field keys (they name option and value IDs) and one
  row per resource and language. Stored data and an API contract.
- **`language` as the parameter,** its fallback order, and "never an error".
- **Regional tags instead of market-specific translations** (ML4).
- **The storefront's URL shape** (search engines index it).
- **Orders' `language` and `localized` snapshots.**
- **Facet keys in the source language** (URLs and saved links hold them).
- **ICU MessageFormat and the catalog keys** (contributors' translations depend on
  them).
- **Message `params`** (a contract with clients, added to, never renamed).

## 16. Layering

- `hypha-translations` is a leaf: core only.
- New edges:
  - `hypha-checkout` → `hypha-translations` (the order's `localized` snapshots).
  - `hypha-search` → `hypha-translations` (documents per language).
- No edge from `hypha-cart`, `hypha-mail` or any leaf: cart responses are overlaid by
  the host, and emails read the order's snapshots.
- Storage adapters implement its repo; `hypha-testkit` gets the in-memory one and a
  contract scenario.

## 17. Build order

1. Core and stores: `languages` on the store and markets, the settings page, countries
   answering them.
2. `hypha-translations`: the aggregate, catalog, repo, contract, Postgres, admin
   routes, cleanup handler, scopes.
3. Storefront reads: `language`, overlays in product, category, custom field
   definition, shipping, payment and rule responses.
4. Carts, orders, customers: `language`, `localized` snapshots.
5. Search: documents per language, facet labels.
6. Admin: the translation editor and worklist.
7. `packages/i18n`; the storefront's strings extracted; URLs, switcher, `hreflang`,
   formats.
8. Emails: template lookup, defaults per language, formats.
9. The admin's strings extracted; `users.language`.
10. Custom field values; the second language's catalogs; the seed's translated content.
