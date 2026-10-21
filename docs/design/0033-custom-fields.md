# 0033: Custom fields

- **Status:** Accepted (2026-10-20). Danilo's answers are in §13.
  - Storage is a row per field, and tightening a validation is refused.
  - Owners in v1 are product, variant, product category, customer and order.
  - Events carry non-personal values, and there's a `json` type.
  - Products keep their tags.
  - `product.deleted` (with product deletion) and `asset.deleted` come first (§6.3,
    §14 step 0).
- **Scope:** typed custom fields on core resources (Shopify's metafields), covering:
  - definitions as data, values and their types
  - who owns and writes them (staff, apps, spores)
  - visibility per surface and personal data
  - list operations, filters and lookups
  - events, the app manifest and spores
  - search facets and the admin's forms
- **Not in scope:**
  - metaobjects (shared typed entries, a later note; §11 reserves them)
  - translations of values (0021 SU5's overlays will cover them)
  - values per market
  - line-item properties that change a cart line's identity (engraving)
  - copying cart fields onto the order (reserved, §11)
- **Related:**
  - 0002 §3 (the first sketch, which this replaces)
  - 0032 §2 and §4 (what customers need; this is built first)
  - 0029 AP7 (app data as fields under the app's namespace) and AP10 (personal data)
  - 0030 SP3 (spores' namespaces)
  - 0019 §9 (extra search document data)
  - 0016 (events)

## Why

Every merchant has data the core model doesn't name:
- a product's material, care guide or ISBN
- a customer's skin type, birthday or loyalty tier
- an order's gift message or ERP number
- an app's own IDs on all of these

Today there's nowhere to put any of it. Apps keep their own databases and can't show
anything on Hypha's objects, and merchants can't add a field without a fork. Customers'
labels (0032 §2) and app IDs such as `app_mailchimp.member_id` wait on this.

Two requirements come from Hypha's goals:
- **Merchants and admin agents add fields at runtime.** That rules out fields as code
  and columns (Vendure, Medusa's modules), since every field would mean a deploy and a
  migration.
- **Everything typed and discoverable.** Admin UIs, agents and generated clients must
  know what exists and what's valid. That rules out an untyped `metadata` blob (Saleor,
  Medusa, Commerce Layer).

## Decisions (proposed)

| | Decision |
| --- | --- |
| CF1 | **One system** for merchants' catalogue data and apps' integration data. Saleor (attributes vs metadata), commercetools (attributes vs Custom Types) and BigCommerce (custom fields vs metafields) each have two, and each pair confuses people. Namespaces and per-definition access cover both uses |
| CF2 | **No value without a definition.** Definitions are data, created through the API, declared in an app's manifest, or registered by a spore. Shopify's untyped legacy metafields are what this avoids |
| CF3 | **Identity is immutable:** owner type, namespace, key and type. Name, description, validations, access and capabilities can change. A different type is a new field |
| CF4 | **Namespaces are owners:** `custom` (the merchant's), `app_<name>` (only that app defines and writes), `spore_<name>` (only that spore). There's one spelling everywhere, unlike Shopify's `$app` / `app` / `app--<id>` |
| CF5 | **Values live outside the aggregates**, one row per (owner, definition) holding typed JSON (Q1). Editing a field never conflicts with editing the product's title, and owners need no migration when fields come and go |
| CF6 | **Lists get item operations** (add, remove: idempotent, done in the database under the row's lock) as well as replacing. Shopify has none, and apps race on read-modify-write |
| CF7 | **Filtering is explicit:** a definition marked `filterable` is indexed and can be filtered on. Filtering on any other field is **rejected** (`custom_field_not_filterable`), never silently ignored as on Shopify |
| CF8 | **Visibility per surface**, opt-in for the storefront. A definition on customers or orders can be `personal`, which redacts it without `customer_data:profile` (0032 CD9) |
| CF9 | **A dedicated event per owner kind** (`product.custom_fields_updated`), carrying what changed. A field write doesn't fire `product.updated` (Saleor moved to this in 3.23) |
| CF10 | **Limits are published from day one and kept small** (§10). Shopify's 2026 cut of value sizes, from megabytes to 16 KB then 128 KB for JSON, broke apps: loosening is cheap, tightening isn't |

## 1. What others do

The research behind this note was done on 2026-10-20 against current docs and source.
The main sources are linked inline.

**Shopify** ([metafields](https://shopify.dev/docs/apps/build/custom-data), [types](https://shopify.dev/docs/apps/build/custom-data/metafields/list-of-data-types), [limits](https://shopify.dev/docs/apps/build/metafields/metafield-limits))
- **Owner types:** around 25, including product, variant, collection, customer, order,
  company, location, market and shop. Cart values copy onto the order through the order
  definition's `cartToOrderCopyable` capability.
- **Types:**
  - text (single and multi line), rich text, integer, decimal (a string), boolean,
    date, date_time, url, color
  - money `{amount, currency_code}`, rating, link, json
  - about 30 measurement kinds `{value, unit}`
  - typed references (product, variant, collection, customer, file, metaobject, mixed)
  - `list.` forms of most of these
- **Definitions:** type, namespace, key and owner type are immutable; key 2–64
  characters, namespace 3–255.
  - Validations: min/max, length, regex, choices (≤ 128), allowed file types,
    metaobject type, JSON Schema for `json`, `list.min`/`max`.
  - **Tightening validations is ambiguous:** some changes fail, and others start an
    async job that flags `SOME_INVALID`.
  - Deleting a definition leaves its values untyped unless asked to delete them.
- **Ownership:**
  - `$app` namespaces are reserved for their app; merchant namespaces are writable by
    every app with the owner's scopes; `shopify--` is reserved.
  - **Access:** `admin` (app-owned only: merchant read, or read and write);
    `storefront` (none by default, or public read; but Liquid sees everything);
    `customerAccount` (none, read, or read and write).
- **Writes:** `metafieldsSet` upserts up to 25 values atomically, with an optional
  `compareDigest` for compare-and-set.
  - **No list append or remove**: apps read, modify, write and race.
  - Values can be set through owner mutations too.
- **Capabilities:**
  - `adminFilterable`: 50 per owner type, 5 for orders. **A filter on an unflagged field
    is silently ignored.**
  - `smartCollectionCondition` and `uniqueValues` (the `id` type, with
    `productByIdentifier`-style lookups since 2025).
  - Storefront filters through Search & Discovery: 25 per store, text, number, boolean
    and metaobject references only.
- **App config:** definitions declared in `shopify.app.toml` under the app's namespace
  are read-only through the API, at most 25 changes per deploy. Merchants edit values
  only when access allows it.
- **Events:** there's no value-change webhook. Apps widen the owner's `products/update`
  with `metafieldNamespaces`, which forums report fires unreliably for variants.
- **Limits:**
  - 256 definitions per owner type per app, and 256 for the merchant.
  - 64 KB per value; lists of up to 128 items.
  - **The 2026 reversal:** a 16 KB cap on every value was announced for 2026-04, then
    revised to 128 KB for JSON only after developers who stored megabytes protested
    ([changelog](https://shopify.dev/changelog/reduced-metafield-value-sizes)).
- **Metaobjects:** typed entries (`custom--color-pattern` with a handle) that values
  reference, so "Navy" is one entry renamed everywhere. Category metafields and option
  values link to them.

**Saleor** ([attributes](https://docs.saleor.io/developer/attributes/overview), [metadata](https://docs.saleor.io/api-usage/metadata))
- **Attributes:** typed (dropdown, multiselect, numeric with a unit, text, rich text,
  boolean, date, file, swatch, reference), assigned through product, page and (since
  3.23.26) customer types.
- **Attributes are stored as EAV:** a value row and a join row per value, even for one
  number.
  - Choice values are shared rows with slugs, and filters use slugs, so they're the same
    in every language.
  - `filterableInStorefront` and three other flags were removed in 3.24 because "Saleor
    never read them".
- **Metadata:** `metadata` (public) and `privateMetadata`, untyped strings with no
  namespacing. Checkout metadata is copied onto the order.
  - **Since 3.23**, a metadata-only change fires the metadata webhook, not
    `ORDER_UPDATED`.

**Medusa v2** ([module links](https://docs.medusajs.com/learn/fundamentals/module-links), [additional data](https://docs.medusajs.com/learn/fundamentals/api-routes/additional-data))
- **Metadata:** a `metadata` JSON on every model.
  - Updates merge shallowly; a key set to `""` is deleted.
  - Only some services merge; the rest replace.
- **Typed data takes code:** a module with its own table, a `defineLink`,
  `additional_data` on routes, a workflow hook to write it, and an admin widget.
  - Merchants can't add a field.
  - Filtering across links needs the experimental Index Module.
  - The admin's `unstable_defineCustomFieldsConfig` works on products only.

**Vendure** ([custom fields](https://docs.vendure.io/guides/developer-guide/custom-fields/))
- **Fields are defined in config, one column per field.** Every change is a migration
  and a deploy.
- **Types:** string, text, int, float, boolean, datetime, struct, relation; `list`,
  `localeString`/`localeText`.
- **Flags:** `public`, `readonly`, `internal`, `requiresPermission`, `unique`,
  `validate`.
- **GraphQL types and list filters are generated.**
- **Order lines:** custom fields on an order line are part of its identity, so two
  engravings make two lines.
- **Names:** plugins share one flat namespace per entity.

**commercetools** ([product types](https://docs.commercetools.com/api/projects/productTypes), [custom fields](https://docs.commercetools.com/api/projects/custom-fields))
- **Product attributes:** typed, with localized text and enum types (`lenum`: stable
  keys, translated labels), a variant or product level, cross-variant constraints, and
  `isSearchable`.
- **Custom Types for 38 other resource kinds.**
  - **Only one Type per resource instance**, so two apps adding fields to an order must
    share one Type.
  - 50 indexed custom fields for customer search.

**BigCommerce** ([metafields](https://docs.bigcommerce.com/developer/docs/integrations/metafields.md))
- Product custom fields: untyped name/value strings, shown on the storefront.
- **Metafields are owned by the API client that made them**, with five permission sets
  (`app_only`, `read`, `write`, `read_and_sf_access`, `write_and_sf_access`). At most
  250 per resource per client; the values are untyped strings.

**Adobe Commerce (Magento)**
- **EAV attributes:** values in one table per backend type, keyed by entity, attribute
  and store view.
- **The joins** led to "flat catalog" tables, which Adobe now discourages
  ([flat catalog](https://experienceleague.adobe.com/docs/commerce-admin/catalog/catalog/catalog-flat.html)).

**What we take:**
- From Shopify:
  - definitions, immutable identity, the type list and money as `{amount, currency}`
  - access per surface, and app namespaces declared in config and read-only through the
    API
  - the `unique` capability for ERP IDs, and filterable as a capability
- From commercetools: choice keys with labels (`lenum`), so filters and integrations
  use keys and labels can be translated.
- From BigCommerce: private app fields.
- From Saleor: a dedicated event, not the owner's update.
- From Vendure: the access flags, generated in OpenAPI.

**What we don't take:**
- Values without definitions, and untyped metadata.
- Fields as columns, and EAV rows per value.
- Two systems.
- Merchant fields writable by any app.
- Silently ignored filters.
- Full-list replace as the only list write.
- Big values.

## 2. Definitions

`CustomFieldDefinition` (`cfd_…`, crate `hypha-custom-fields`, a leaf):

| Field | |
| --- | --- |
| `owner_type` | `product`, `variant`, `category` (product categories), `customer`, `order` in v1 (Q3); more later with no model change |
| `namespace` | `custom`, `app_<name>` or `spore_<name>` (CF4) |
| `key` | `[a-z0-9_]{2,64}`, starting with a letter; addressed as `namespace.key`, e.g. `custom.material` |
| `name`, `description` | For forms (name ≤ 255 characters, description ≤ 1,000) |
| `type` | §3 |
| `validations` | Per type (§3) |
| `access` | §5 |
| `personal` | Only on `customer` and `order` owners (`personal_not_allowed` otherwise): the value is about the person (§5.2) |
| `capabilities` | `filterable` (§7), `unique` (§7; text, integer and url only) |
| `position` | Order on the owner's form, dense within an owner type like category siblings |

- **Unique keys:** one definition per (owner type, namespace, key). Repo key
  `CUSTOM_FIELD_KEY`, mapped to `custom_field_exists`, read-then-check plus the
  `Duplicate` mapping as everywhere.
- **Changing validations.** A change that only loosens is applied. A change that
  tightens (a lower `max`, fewer `choices`, a `pattern` added, `unique` turned on) is
  checked against the stored values first. If any break it, the change is refused with
  `custom_field_values_invalid`, giving `details: {count, owners: [first 20 IDs]}`, so
  staff fix those values and try again (Q2).
  - The check runs in the command, page by page over the definition's values. Changes
    to definitions are rare, and a definition holds one row per owner.
  - Stored values therefore always pass their definition. A value is only ever parsed
    by its type when loaded (type is immutable), so tightening can't make rows fail to
    load (CLAUDE.md's `restore` rule).
- **Deleting a definition** deletes its values in the same transaction (a cascade in
  the module's own tables) and records `custom_field_definition.deleted`. There's no
  Shopify-style "keep them untyped", since untyped values don't exist (CF2).
- **Events:** `custom_field_definition.created`, `.updated {fields}`, `.deleted`, each
  with `{definition, owner_type, namespace, key}`.
- **Scopes:**
  - `custom_fields:read` and `custom_fields:write` cover definitions in `custom`.
  - Values are governed by the owner's scopes (§5).
  - Apps and spores manage their own definitions through their manifest and code, not
    through these routes (§8).

## 3. Types

| Type | Value (JSON) | Validations |
| --- | --- | --- |
| `text` | string, one line, ≤ 255 characters | `min_length`, `max_length`, `pattern` (RE2-style via the `regex` crate, which has no backtracking) |
| `multiline_text` | string, ≤ 16 KiB | `min_length`, `max_length` |
| `rich_text` | HTML string, sanitised like core's `Description` (≤ 16 KiB here) | `max_length` |
| `integer` | number, within ±(2⁵³ − 1) like `Money::MAX_AMOUNT` | `min`, `max` |
| `decimal` | string `"12.50"` (exact; no floats in Hypha) | `min`, `max`, `max_scale` |
| `boolean` | `true` / `false` | — |
| `date` | `"2026-10-20"` | `min`, `max` |
| `date_time` | RFC 3339, stored as UTC | `min`, `max` |
| `url` | `https:`/`http:`/`mailto:`/`tel:` URL, ≤ 2,048 characters | `schemes`, `domains` |
| `money` | `{"amount": 1999, "currency": "EUR"}`: integer minor units, Hypha's money shape | `currencies`, `min`, `max` (in minor units, same currency) |
| `measurement` | `{"value": "1.5", "unit": "kg"}` | `dimension` (required: `weight`, `length`, `volume`), `min`/`max` (converted) |
| `choice` | the choice's key, `"dry"` | `choices: [{key, label}]` (required, ≤ 128; keys `[a-z0-9_-]{1,64}` and immutable, labels editable) |
| `reference` | a prefixed ID, `"prod_…"` | `kind` (required: `product`, `variant`, `category`, `customer`, `asset`; more later) |
| `json` | any JSON, ≤ 16 KiB serialised | none in v1; a JSON Schema later (Q5) |
| `list.<t>` | an array of `t`, for every type above except `boolean`, `rich_text`, `multiline_text` and `json` | `t`'s, plus `min_items`, `max_items` (≤ 128), `unique_items` (case-insensitive for text) |

Notes on these types:

- **`choice` stores keys, never labels.** Filters, search facets and integrations use
  keys, so renaming "Dry skin" to "Dry" breaks nothing, and labels can be translated
  later (commercetools' `lenum`, Saleor's slugs).
- **`reference` targets:**
  - A reference is checked to exist, with the right kind, when it's written.
  - It's weak: there's no foreign key, since one column can't point at four tables.
  - When the target is deleted, a cleanup handler removes it (§6.3), and readers
    tolerate a dangling ID in between.
- **Measurement units:**
  - The fixed set is `g`, `kg`, `oz`, `lb`, `mm`, `cm`, `m`, `in`, `ft`, `ml`, `l`,
    `fl_oz`.
  - Conversions are exact decimal ratios, used for `min`/`max` and range filters.
- **Not in v1:**
  - `color` (a `text` with a `pattern` covers it)
  - `rating` and `link`
  - references to entries (metaobjects, §11)
  - localized text (translations are overlays, 0021 SU5)
- **A value type per field type in Rust:** `FieldValue` is an enum, parsed from JSON
  by the definition's type and validations. `FieldValue::parse(&FieldType,
  &Validations, serde_json::Value) -> Result<FieldValue, ValueError>` is the one place
  values are checked. Every error carries the path, e.g. `custom.sizes[3]`.

## 4. Values and writes

**Storage (Q1):**

```sql
custom_field_values (
  definition_id uuid  REFERENCES custom_field_definitions ON DELETE CASCADE,
  owner_id      uuid,                    -- the product, variant, customer or order
  value         jsonb NOT NULL,
  scalar_text   text,                    -- lowercased text / choice / url / reference, for filters and `unique`
  scalar_num    numeric,                 -- integer / decimal / money / measurement in base units / dates as epoch
  version       bigint NOT NULL,
  created_at, updated_at,
  PRIMARY KEY (definition_id, owner_id)
)
```

The table's indexes:
- an index on `owner_id`, for loading an owner's fields
- a GIN index on `(definition_id, value)` (`btree_gin`), for list `contains` and equality
- B-trees on `(definition_id, scalar_text)` and `(definition_id, scalar_num)`, where
  the definition is filterable
- a partial unique index on `(definition_id, scalar_text)` where `unique`

The owner type comes from the definition, so the row doesn't repeat it.

**Why a row per field and not one document per owner:**
- Two apps writing different fields of one product never conflict.
- A list delta locks one row.
- Deleting a definition is one `DELETE`.
- Each field has its own version.

**Why it isn't the EAV trap:**
- There's one row per field, not per value, and a list is one row.
- Values are typed JSON, not one table per type.
- Loading an owner's fields is one query: `WHERE owner_id = ANY($1)` for a whole page,
  batched like fixed prices, pipelined with the owner's own load.

**Commands** (all take the owner already checked to exist, §6.1):

- **`set_custom_fields(ctx, owner, {"custom.material": "wool", "custom.care": null})`**
  - Merge-patch: a key absent is kept, `null` removes the value.
  - At most 50 keys per call.
  - It checks the caller may write each key (§5), parses every value, and reports
    **every** problem at once (`custom_fields_invalid`, `details: [{key, path, code,
    message}]`).
  - It stages the changed rows plus one event in one batch: all or nothing, like
    Shopify's `metafieldsSet`.
- **`change_list_items(ctx, definition, owners, {add: [...], remove: [...]})`** (CF6)
  - For one list field on 1–250 owners: the 0032 §2 bulk route, which one owner also
    uses.
  - Done in SQL under the row's lock, so it's idempotent and concurrent writers both
    land: adding an item that's present (case-insensitive for text with
    `unique_items`), or removing one that isn't, changes nothing and records nothing.
  - `max_items` is checked after the change, and an owner that would exceed it fails
    the whole call (`too_many_items`, with the owner).
- **`set_field_values(ctx, definition, [{owner, value}])`**: one field on up to 250
  owners. It's for imports and the admin's "set on selected".
- **Optimistic writes:** each value has a `version`, and `set_custom_fields` takes an
  optional `if_versions: {"custom.material": 3}` (`custom_field_stale`, 409). It's
  optional because most writers are fine with the last write winning, and list deltas
  don't need it.

**Routes:**

| Route | |
| --- | --- |
| `GET`/`PATCH /admin/{products,variants,categories,customers,orders}/{id}/custom-fields` | Read with versions, and `set_custom_fields` |
| `POST /admin/custom-fields/{definition}/items` | `change_list_items` over owners |
| `POST /admin/custom-fields/{definition}/values` | `set_field_values` |
| `GET`/`POST /admin/custom-field-definitions`, `GET`/`PATCH`/`DELETE …/{id}` | Definitions (`?owner_type=`) |
| `GET /admin/custom-field-definitions/{id}/values-in-use` | Distinct values with counts, for autocomplete (filterable text, choice and list fields; 0032 §2) |
| `GET /admin/custom-field-definitions/suggestions` | The well-known fields worth having (0032 CD11: customers' `custom.tags`, `custom.note`), each as the `POST` body that creates it plus `why` and `existing` (the merchant's definition of that key, if any); `?owner_type=` |

Variants use `/admin/variants/{id}` because a variant ID is global. The owner responses
carry `custom_fields` too (§6.2).

## 5. Access

### 5.1 Who reads and writes

| | Read | Write |
| --- | --- | --- |
| `custom.*` | Callers with the owner's read scope (`products:read`, …) | Callers with the owner's write scope |
| `app_<name>.*` | The owner's read scope, unless the definition is `private` (BigCommerce's `app_only`): then only that app and staff | Only that app (`Actor::App` with its `AppId`). Staff too when `access.admin` is `read_write` (Shopify's knob); other apps never |
| `spore_<name>.*` | As apps | Only the spore's code (its `SporeEnv` writes as the spore) and staff when `access.admin` is `read_write` |
| Storefront surface | Only definitions with `access.storefront: read` | Nobody in v1. `read_write` is reserved for cart fields (§11) |

Merchant fields stay writable by any caller with the owner's write scope, as on
Shopify. The difference is that an app can't create fields in `custom`. It declares its
own, so a stray app writes to its own namespace, and staff can see which app owns
which field.

### 5.2 Personal data (CF8)

- A `personal` definition (customers and orders only) is redacted by `redact.rs`
  unless the caller has `customer_data:profile` (0032 CD9).
  - The value is removed from `custom_fields` and listed in `redacted` as
    `custom_fields.custom.birthday`.
  - Personal values are also left out of the events' payloads, which carry only the key
    (§6.4).
  - Filtering or sorting by a personal field needs the scope (`require_fields`, as with
    emails).
- **Order fields aren't personal by default.** An ERP number isn't about a person, but
  a gift message to "Grandma Ruth" is, and its definition should say so. The admin's
  form asks.
- **`redact.rs` learns the personal keys per request.** The route passes the owner
  type's personal definition keys (loaded once with the definitions, §6.2), since
  they're data, not a static list.
- **0032's erasure** removes a customer's personal values and the personal values on its
  orders in its batch.

## 6. Owners, reads and events

### 6.1 Owners

- **The leaf can't see products or customers.** Its commands take an
  `Owner { kind, id }` the route has already checked: it loads the product, category,
  customer or order through its own repository first (unknown is 404, as for the owner itself).
  It's the same shape as extension calls, where routes load the app first.
- A **`variant`** owner is checked through its product.
- Other workflows that already hold the owner (checkout, erasure) pass it directly.

### 6.2 Reads

**Single owners:**
- A single owner's response (`GET /admin/products/{id}`, a customer, an order) carries
  `custom_fields: {"custom.material": "wool", …}`.
- It holds every value the caller may read (§5), loaded with the owner's other side
  reads (channels, categories) in the same pipelined round trip.
- Variants in a product response carry their own `custom_fields`.

**Lists:**
- Lists carry none by default, which keeps pages cheap.
- `?custom_fields=custom.material,custom.tags` (≤ 10 keys) adds those, one batched
  query per page. It's handy for tables and agents.

**Definitions:**
- The owner's definitions are cached per request: loaded once, used for parsing,
  access and redaction.
- On Workers the isolate may keep them for a few seconds, keyed by a definitions
  version. A small cost: definitions are few per owner type.

**Storefront:**
- `StorefrontProductResponse` and its variants carry `custom_fields` with only the
  `storefront: read` definitions.
- Storefront category responses carry their `storefront: read` fields the same way.
- `GET /storefront/custom-field-definitions` (`?owner_type=`; added after the browser
  test showed names guessed from keys) lists those definitions: field, owner type, name,
  description, type, position, and a choice's labels. Storefronts name, order and label
  values by them. Customers' and orders' fields never appear, since storefronts don't
  read those owners.
- The storefront's revalidation route expires `product:<handle>` on
  `product.custom_fields_updated`, `category:<handle>` on
  `category.custom_fields_updated`, and the definitions (tag `custom-fields`) with the
  pages of their owner type on definition changes a shopper can see (name, description,
  access, validations, capabilities, position).

### 6.3 Owner deletion and references

- **The values have no foreign key to their owners.** The owners live in other modules,
  in four tables. An in-process handler, `CustomFieldCleanup` in `hypha-custom-fields`,
  wants:
  - `product.deleted` and `product.variant_removed`
  - `customer.deleted`
  - `order.deleted` (if orders ever are)
  - `category.deleted` and `asset.deleted`, to clear references
- It deletes those owners' values and removes references to deleted targets: one item
  from a list, or the whole value. It records the usual events.
- **Today only some of these events exist.** `product.variant_removed` and
  `category.deleted` are recorded now. `customer.deleted` comes with 0032 §7.2. The two
  that are missing come first (§14 step 0):
- **`asset.deleted {asset}`**
  - `delete_asset` records it in the same write as the row (the repo's `delete` takes
    the asset's events, as `insert` and `update` do).
  - References to assets stay weak in v1. `delete_asset` already refuses an asset in
    use by products and categories (asset usages, 0014), but it can't see field
    values. A field reference that's cleared when its asset goes is the price of
    keeping the leaf independent of `hypha-media`.
  - Making field references asset usages ("used by: Product X, field
    `custom.size_chart`") is a later step through a workflow.
- **`product.deleted {product, handle, variants: [...]}`**, with product deletion
  itself. Today a product can only be archived. Deletion follows Shopify:
  - **The command:** `DELETE /admin/products/{id}` (`products:write`), via
    `hypha_merchandising::delete_product`. Merchandising is the composing crate that
    already writes a product together with its asset usages.
  - **One batch:** it removes the product (variants and options go with it, since the
    product owns them) and its asset usages, and records the event.
  - **What stays:** the assets themselves.
  - **Orders keep their lines.** They snapshot titles, SKUs and prices, so a deleted
    product changes no order.
  - **Carts:** a line whose product is gone is reported unavailable when the cart is
    priced, the way a line that's no longer sold is, and checkout refuses it until
    it's removed.
  - **Cleanup by event, like a removed variant:**
    - `VariantCleanup` also wants `product.deleted` and cleans each listed variant:
      fixed prices, and inventory items (retired while units are reserved, 0018).
    - `SearchIndexer` drops the product's documents.
    - `CustomFieldCleanup` deletes its fields and the variants' fields.
    - Publications and category memberships go with the product's row where storage
      cascades them, or else through those crates' own handlers. The build checks
      which, table by table.
  - **The storefront** expires `product:<handle>` and the listings on
    `product.deleted`.
- It's eventually consistent, like search sync. Readers already ignore values whose
  owner doesn't load.

### 6.4 Events (CF9)

There's one event type per owner kind:
- `product.custom_fields_updated` (variant values are reported here too, with
  `variant`)
- `category.custom_fields_updated`
- `customer.custom_fields_updated`
- `order.custom_fields_updated`

```json
{ "product": "prod_…", "variant": null,
  "changes": [
    { "key": "custom.material", "value": "wool" },
    { "key": "custom.tags", "added": ["vip"], "removed": [] },
    { "key": "custom.birthday" }
  ] }
```

- **What a change carries:**
  - **Non-personal fields** carry the new value, or the delta for list operations.
    That's what a Mailchimp-style sync needs (0032 §2), and Shopify has no value event
    at all.
  - **Personal fields** carry only the key.
  - **Values over 1 KiB** carry only the key, so payloads stay small.
- **A write that changes nothing records nothing**, as with every other event.
- **Delivery:**
  - Apps subscribe to these types like any other.
  - When an event is delivered, entries the receiving app may not read are dropped
    (another app's `private` field, a personal field without the scope), alongside the
    existing redaction.
  - A delivery left with no entries isn't sent.
  - Subscribing by namespace (`custom_fields_updated` for `app_mine.*` only) is a
    later filter; v1 apps ignore keys they don't want.
- **`product.updated` doesn't fire for field writes.** Search and the storefront listen
  to the new type where they need it.

## 7. Filters and lookups (CF7)

- A definition with `capabilities.filterable` (at most 50 per owner type) gets its
  `scalar_*` B-tree indexes used, and becomes a list filter on its owner's route.
- **Filter syntax:**
  - `field[custom.material]=wool,cotton`: any of these.
  - `field[custom.tags]=vip`: a list contains it.
  - `field[custom.weight][gte]=1.5&field[custom.weight][lt]=3`: integer, decimal,
    money, measurement, date and date_time, with `gt`/`gte`/`lt`/`lte`.
  - `field[custom.flag]=true`.
  - `field[custom.erp_id][exists]=false`.
- **Rejected filters:** a filter on a non-filterable field, an unknown key, or a
  personal field without the scope is a **422** (`custom_field_not_filterable`,
  `custom_field_unknown`, the scope error), never ignored.
- **OpenAPI** documents the pattern once per list route, since the keys are data.
- **Lookup by value** goes through the same filter: an app's
  `field[app_erp.id]=A-1001` with `unique` returns at most one owner. It's Shopify's
  `productByIdentifier` without a route per owner, and enough for ERP sync and for
  agents.
- **Sorting** by a filterable scalar field (`sort=field:custom.weight`) is reserved: it
  needs keyset paging on a value that may be absent (0024's sorted lists), so it's a
  later step.

## 8. Apps and spores

**Apps declare their fields in the manifest** (Shopify's TOML):

```json
"custom_fields": [
  { "owner_type": "customer", "key": "member_id", "type": "text", "name": "Mailchimp member",
    "access": { "admin": "read", "storefront": "none" }, "private": false,
    "capabilities": { "unique": true, "filterable": true } }
]
```

- **The namespace is always the app's.** A manifest can't name one; `key` is enough.
- **When the definitions change:**
  - They're created when the app is installed (approved), and updated when a refreshed
    manifest is approved (0029 §4).
  - Through the definitions API they're read-only (`custom_field_managed`, 409).
  - A manifest that changes an existing key's type is invalid (`manifest_invalid`, at
    `custom_fields[i].type`), since the type is immutable.
  - A key removed from the manifest is deleted with its values when the refreshed
    manifest is applied. The consent screen lists "removes fields: …" so staff see it
    coming.
- **Staff see an app's fields.** The consent screen lists which owners the app adds
  fields to, and whether they're personal: a personal field needs the app to ask for
  `customer_data:profile`.
- **On uninstall** the definitions and values stay, readable by staff. The app's
  second `DELETE` (removal) deletes them: `custom_field_definitions.app` has a foreign
  key to `apps(key)` with `ON DELETE CASCADE`, like `shipping_provider_app`. Removal
  says so in the admin.
- **The manifest's JSON Schema** (`docs/apps/manifest.schema.json`) gains
  `custom_fields`, generated from `ManifestDocument`, and `@hypha/app`'s types follow.

**Spores** register their fields in code:
- `Spore::custom_fields() -> Vec<FieldSpec>`, in the `spore_<name>` namespace.
- They're synced at startup by the host, next to `register_spore`, under the same
  rules: a type change refuses to start, naming the field, and a removed field is
  deleted.
- A spore reads and writes its values through `SporeEnv` as the spore.

## 9. Search facets and the admin

**Search** (0019 §9's seam, made concrete):
- **Which fields can be facets:** product and variant definitions that are
  `filterable` with `access.storefront: read`, of type `text`, `choice`, `boolean`,
  `integer` or their lists.
- **The facet:** `FacetKey::Field(definition)`, written `field.<namespace>.<key>`.
  Merchants pick it in search settings like options and vendors, within the existing
  `MAX_FACETS`.
- **Documents:** `build_documents` takes the owner's storefront-visible values. A
  document's `facets` array gains `field.custom.material` + `wool` entries, so the
  Postgres engine needs no schema change. Variant values win over product values for
  the same key.
- **Rebuilds:** `SearchIndexer` also wants `product.custom_fields_updated` and the
  definition events that change storefront access or `filterable`.
- **Labels:** a `choice` facet shows its labels; filters use keys.

**The admin:**
- **Settings → Custom fields:** per owner type, a list of definitions. Merchant ones can
  be edited. App and spore ones are read-only, named by their app.
- **A "Custom fields" card on product, variant, customer and order pages:**
  - It renders each definition by type: text inputs, a choice select, a list editor
    with chips, money with the currency, a date picker, a reference picker reusing the
    product, category and customer pickers.
  - Fields the session can't write are shown read-only.
  - It joins the product editor's draft and save bar: a `PATCH …/custom-fields` after
    the product's own writes, as categories do.
- **Shared rendering:** the renderer is written once (`components/custom-fields/`) and
  later also serves app action inputs. Today `schema-form.ts` and `spore-settings.tsx`
  each render a flat schema. Merging the three is a follow-up, not a v1 requirement.
- **List filters:** the products, categories, customers and orders lists offer their filterable
  fields as filters.

## 10. Limits (CF10)

Constants, in `hypha-custom-fields`' `limits.rs`, documented in the API:

| | |
| --- | --- |
| Definitions per owner type, per namespace | 128 |
| Filterable definitions per owner type (all namespaces) | 50 |
| Keys per `set_custom_fields` call | 50 |
| Owners per bulk call | 250 |
| `text` | 255 characters |
| `multiline_text`, `rich_text`, `json` | 16 KiB |
| Any value, serialised | 64 KiB |
| List items | 128 |
| `choices` | 128 |
| Facets from fields | within search's `MAX_FACETS` (20) |

Shopify's limits are higher. These can rise later without breaking anyone, and a
merchant who needs a document should store it as a file (an asset reference).

## 11. Later (reserved)

- **Cart fields copied to the order:**
  - Cart becomes an owner type, and its definitions take `access.storefront:
    read_write` (a gift message entered at checkout).
  - An order definition with the same key and `copy_from_cart` gets the value when
    checkout places the order, in its batch (Shopify's `cartToOrderCopyable`, Saleor's
    copied metadata).
  - Line-item properties that change a line's identity (Vendure's order-line fields)
    are a separate, later design.
- **Metaobjects:** shared typed entries (`entry` references: a "Navy" colour pattern, a
  size chart, an author) with their own definitions, built on the same types and
  validator.
- **Translations** of `text`, `multiline_text`, `rich_text` and choice labels, through
  0021 SU5's overlays: (resource `custom_field_value`, field, language).
- **More owner types:** market, location, store, company (0032 §10).
- **Sorting by a field, a JSON Schema for `json`, and `rating`/`link`/`color` types.**
- **Event subscriptions filtered by namespace.**
- **Customer-account access** (`access.customer_account: read | read_write`) with
  customer accounts.

## 12. Changes to existing crates

- **`hypha-core`:**
  - scopes `custom_fields:read`, `custom_fields:write` and `customer_data:profile`
  - ID `CustomFieldDefinitionId` (`cfd_`)
- **`hypha-custom-fields` (new leaf):**
  - definitions, types, `FieldValue` and the validator
  - `DefinitionRepo`, `ValueRepo` and `StageCustomFieldValues`
  - commands, events, `CustomFieldCleanup`, limits
  - contract scenarios in `hypha-testkit` (`custom_field_definitions`,
    `custom_field_values`: versions, the definition key, `unique`, list deltas under
    concurrency, cascades)
- **`hypha-store-postgres`:** the module's migrations (`btree_gin`), repos, batch
  staging, list deltas in SQL, and filter predicates appended to the products,
  categories, customers and orders list queries.
- **`hypha-apps`:**
  - the manifest's `custom_fields`, with checks and consent lines
  - the definition sync on approval and refresh
  - the cascade on removal
- **`hypha-spore-core`:** `Spore::custom_fields()`.
- **`hypha-search-index` / `hypha-search`:** `FacetKey::Field`, the documents' field
  values, and the handler's new event.
- **`hypha-http`:**
  - the routes, responses' `custom_fields` and `?custom_fields=`
  - `field[...]` filters
  - `redact.rs`'s profile family with per-request personal keys
  - webhook entry filtering
  - OpenAPI
- **The products, categories, customers and orders crates:** none. Their list filters gain an
  opaque `fields: Vec<FieldFilter>` from core, which adapters turn into SQL. Their
  aggregates stay untouched (CF5).
- **Hosts:** `CustomFieldCleanup` in `handlers.rs`.
- **Apps:**
  - the admin's settings page, field card, list filters
  - the SDK and `@hypha/app`'s manifest types
  - the storefront's revalidation case

## 13. Decisions on the questions (2026-10-20)

- **Q1. Storage: a row per (owner, field).** Recommended and chosen (§4).
- **Q2. Tightening a validation while stored values break it: refused,** listing the
  first 20 owners to fix (§2).
- **Q3. Owner types in v1:** product, variant, customer and order, **plus product
  categories.**
  - Categories have no `metadata` or `external_id` (0022 waited for this), and their
    pages want merchant content: a banner text, SEO copy, a size chart reference.
  - Their events and storefront tags already exist (`category.changed`,
    `category:<handle>`), so the extra cost is small.
- **Q4. Events carry non-personal values: yes** (§6.4).
- **Q5. A `json` type in v1: yes,** capped at 16 KiB, with no schema yet.
- **Q6. Products keep their `tags`** for now. They may be removed later in favour of a
  list field. They were an early design and nothing else depends on them being core.
- **Also decided:** add `product.deleted` (with product deletion) and `asset.deleted`
  (§6.3) before the custom fields work.

## 14. Build order

Each step is a commit series with its tests, OpenAPI, SDK and e2e steps.

0. **The two missing deletion events** (§6.3):
   - `asset.deleted` from `delete_asset`.
   - Product deletion: `hypha_merchandising::delete_product` and
     `DELETE /admin/products/{id}`, recording `product.deleted`.
     - `VariantCleanup` and `SearchIndexer` want it, publications and category
       memberships go with the product, and carts report its lines unavailable.
     - The admin gets a delete button next to archive. The storefront revalidates on
       the event.
   - Both are useful on their own, before custom fields exist.
1. **The leaf:** IDs and scopes in core; `hypha-custom-fields` with definitions, types,
   `FieldValue`, the validator, commands and events; the in-memory repos and contract.
   This is the bulk of the rules, with fast tests.
2. **Postgres:** the migrations, repos, batch staging, list deltas and the contract
   against Postgres.
3. **Routes:**
   - the definitions and values routes for products, variants, categories, customers
     and orders, and `custom_fields` on their responses
   - the bulk routes
   - redaction of personal fields and webhook entry filtering
   - `CustomFieldCleanup`
   - e2e steps
4. **Filters:** `field[...]` on the products, categories, customers and orders list
   routes, `unique` lookups,
   `values-in-use`.
5. **Admin:** settings → custom fields, the field card on products, categories,
   customers and orders, list filters.
6. **Apps and spores:** the manifest's `custom_fields`, the consent lines, the sync and
   cascade; `Spore::custom_fields()`; `@hypha/app`.
7. **Storefront and search:** storefront responses, revalidation, field facets.
8. **Then 0032's customer steps**, with labels as `custom.tags`-style list fields.
