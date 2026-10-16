# 0015: Shipping

- **Status:** Accepted (2026-10-15). Danilo answered the open decisions in section 10
  the same day; answers are marked *Decided*. Danilo set the direction:
  shipping comes before fulfilment, and **rate providers must be pluggable**: built-in
  rates, and providers added by others, including custom ones as spores.
- **Scope:** where a store ships (zones), what it offers there (shipping options and
  their prices), rates calculated by providers, the shipping address and the chosen
  option on a cart, shipping in cart totals and tax, and the shipping snapshot on an
  order.
- **Not in scope:** fulfilment (shipments, labels, tracking, `fulfillment_status`,
  deducting stock: note 0018), local pickup and local delivery (section 9), returns,
  billing addresses (with payments), duties and import taxes, the outbox (0016).
- **Related:** 0002 (spores, ports, external apps), 0003 (tax: shipping tax was left
  for this note), 0004 (locations), 0005 (`Address`), 0007 (orders, O4 snapshots;
  section 10 listed shipping), 0010 (surfaces), 0013 (the port-and-adapter pattern
  media used before spores exist), UCP's fulfilment extension.

## Decisions

| | Decision |
| --- | --- |
| SH1 | **A leaf crate `hypha-shipping`** owns zones, shipping options and the rate port. Carts and checkout compose it, as they compose tax |
| SH2 | **A shipping zone** (`szone_…`) is a named set of destination countries. Options belong to a zone; a cart sees the options of the zone its shipping country is in. Provinces and postal codes in zones come later, additively (section 2) |
| SH3 | **A shipping option** (`shipopt_…`) is what a buyer picks ("Standard", "Express"): a name, a delivery estimate, and **how its price is found**: a built-in rate, or a provider's (section 3) |
| SH4 | **Built-in rates** are computed by Hypha with no I/O: flat, free over a subtotal, or tiers by subtotal or by weight. Prices are per currency, like price lists (Q8, *Decided*) |
| SH5 | **Providers calculate rates behind one port, `RateProvider`**, through three kinds of provider: Hypha's built-in rates, **external apps over HTTP** (a signed rate callback, like Shopify's CarrierService, so a TypeScript developer adds a carrier without Rust), and **Rust spores** (in-process). An option names its provider by key. *Hard to reverse* (Q1, *Decided*) |
| SH6 | **The rate callback is a published contract**: Hypha POSTs a signed JSON rate request and gets rates back, within a timeout. Its shape and signature scheme are what external carrier apps are written against. *Hard to reverse* (section 5) |
| SH7 | **A provider that fails or times out never fails the cart**: its options are left out with a reason, and the merchant can mark an option as the backup shown when every provider fails (section 6) |
| SH8 | **Carts get a shipping address and delivery groups**: lines that need shipping form a group with its offered options and the buyer's choice. One group for now; the list mirrors UCP's `groups[]` so split shipments are additive. *Hard to reverse* (Q3, *Decided*) |
| SH9 | **Calculated rates are quoted and kept on the cart** for 15 minutes. Checkout re-quotes an expired quote and refuses a changed price with the existing `total_changed`, so a buyer never pays a price they weren't shown (Q6, *Decided*) |
| SH10 | **Shipping is taxed like a line**: `TaxableLine` gains a kind; an option may name a tax class, otherwise the region's default rate applies. Shipping prices are inclusive or not as the market's prices are (0003, T1) (Q7, *Decided*) |
| SH11 | **Facts about a product live on it; policies about it live in the domain that applies them.** Variants gain `weight` and `requires_shipping` (facts: customs and spec sheets read weight too); shipping profiles, when they come, live in `hypha-shipping` linked by ID, like tax classes. Shipping never reads products: the cart passes plain values in. Inventory locations gain an address (the origin of a rate request) (Q4, *Decided*) |
| SH12 | **Orders snapshot shipping**: the address, and per group the option's name, code, provider, price and tax. Totals gain `shipping_total`. Nothing is re-quoted after checkout (0007, O4) |

## 1. What others do

**Shopify** ([delivery profiles](https://shopify.dev/docs/apps/build/purchase-options/deferred/delivery-and-deferral/delivery-profiles), [`CarrierService`](https://shopify.dev/docs/api/admin-graphql/latest/objects/DeliveryCarrierService))
- Delivery profiles group products; each profile has location groups, each with zones
  (countries and provinces), each with method definitions.
- A method's rate is flat, or conditional on order price or weight, or **carrier
  calculated**: Shopify POSTs a rate request (origin, destination, items with grams,
  price and quantity, currency) to the app's callback URL and shows the rates it returns
  (`service_name`, `service_code`, `total_price`, delivery dates). Backup rates cover a
  carrier that fails.
- Weight and `requiresShipping` are per variant (on its inventory item).
- An order has `shippingAddress` and a list of `shippingLines`.

**Medusa v2** ([fulfillment module](https://docs.medusajs.com/resources/commerce-modules/fulfillment))
- A fulfilment set (shipping or pickup) per stock location, with service zones (country,
  province, city, postal code), each with shipping options.
- An option's price is `flat` or `calculated`; calculated ones ask its **fulfilment
  provider** (a module provider: `calculatePrice`, `validateOption`,
  `createFulfillment`, …). Shipping profiles group products; a cart gets one shipping
  method per profile.

**UCP** ([fulfilment extension](https://ucp.dev/specification/shopping/extensions/fulfillment/))
- Checkout `fulfillment.methods[]`: a `type` (`shipping`, `pickup`), the
  `line_item_ids` it covers and a destination; each method has `groups[]` (packages),
  each with `options[]` (`id`, `title`, `totals[]`, `earliest_fulfillment_time`,
  `latest_fulfillment_time`) and a `selected_option_id`.
- A rejected choice leaves the checkout unchanged and returns a `recoverable` message
  whose `path` points at the field.

**What we take**
- Shopify's **rate callback** for external apps: the tier TypeScript developers use (0002
  §1), and a contract they already know from Shopify.
- Medusa's **provider per option** for in-process spores.
- UCP's **groups** as the shape of a cart's shipping, so the UCP adapter maps one to one.
- Weight and "requires shipping" **per variant**, like Shopify.

**What we leave out for now**
- Shopify's delivery profiles and Medusa's shipping profiles (product groups with their
  own rates): a later, additive step (section 9).
- Zones per location group: one set of zones, origins from locations (section 4).

## 2. Zones (`hypha-shipping`)

| Field | Notes |
| --- | --- |
| `id` | `szone_…` |
| `name` | "Domestic", "EU" (shared text rule) |
| `countries` | One or more; **a country is in at most one zone** (key `shipping_zone_country`), so a destination finds one set of options |

- A zone isn't tied to a market: markets decide currency and tax, zones where parcels go.
  A cart in market *EU* shipping to Croatia sees the options of whichever zone holds HR.
- A country with no zone gets no options: checkout of a cart that needs shipping is
  refused (`no_shipping_to_country`, 409), and the storefront can say so before checkout.
- Later, additively: provinces and postal-code patterns inside a zone, with the most
  specific match winning.

## 3. Shipping options

| Field | Notes |
| --- | --- |
| `id` | `shipopt_…` |
| `zone` | The zone it is offered in |
| `name` | What the buyer sees ("Express") |
| `code` | Stable, for apps and analytics (`express`), unique within the zone |
| `rate` | `Builtin(BuiltinRate)` or `Provider { provider, service }` |
| `estimate` | Optional delivery days, `min`–`max` (a provider's quote can override them) |
| `tax_class` | Optional (SH10) |
| `is_backup` | Offered only when no regular option of the zone is (disabled, failed or missing alike) (SH7) |
| `enabled`, `position` | Order of display |

**Built-in rates** (SH4). *Changed while building:* the four kinds are one type,
`BuiltinRate { basis, tables }`: a **basis** (`flat`, `subtotal` or `weight`) and a
**table of tiers per currency** (one table per currency, like a price list; a cart in a
currency the option has no table for doesn't see it). A tier's price applies while the
basis is below its `up_to` (exclusive); the last tier is open. The admin offers the four
kinds as presets of it:

- **Flat:** basis `flat`, one open tier per currency.
- **Free over a subtotal:** basis `subtotal`, a tier up to the threshold, then a free one.
- **Subtotal tiers** and **weight tiers:** basis `subtotal` or `weight`, any number of tiers.
- A weight basis can't price lines whose weight is unknown: such a cart doesn't see the
  option (reason `weight_unknown`).

**Provider rates:** `{ provider: ProviderKey, service: String }`. The option offers that
one service of that provider, under the merchant's name. A provider's list of services
comes from `GET /admin/shipping-providers/{key}/services` so the admin can offer a
picker (Q2).

## 4. What a rate request carries

```rust
pub struct RateRequest {
    pub currency: Currency,
    pub origin: Option<Address>,       // the shipping location's address (SH11)
    pub destination: Address,          // the cart's shipping address
    pub items: Vec<RateItem>,          // the group's lines
    pub subtotal: Money,               // the group's lines, as priced
}
pub struct RateItem {
    pub variant: VariantId,
    pub sku: Option<String>,
    pub quantity: Quantity,
    pub unit_price: Price,
    pub weight: Option<Weight>,        // grams, u32
}
pub struct Rate {
    pub service: String,
    pub name: String,
    pub amount: Money,                 // in the request's currency
    pub estimate: Option<DeliveryEstimate>,
    pub reference: Option<String>,     // the provider's quote ID, for buying the label later (0018)
}
```

- **Origin:** the location that would ship the group: the first location of the market,
  by priority, that covers every line (0004, I9). Locations gain an optional `address`
  (SH11); a provider that needs an origin and gets none returns an error for that
  request (SH7).
- **Weight:** variants gain `weight: Option<Weight>` (grams) and
  `requires_shipping: bool` (default `true`). A cart's lines that don't require shipping
  (digital goods, gift cards) form no group, and a cart of only those needs no address.

## 5. Providers (SH5, SH6)

```rust
/// Calculates rates for options priced by a provider. The host implements it once,
/// dispatching by key to the providers it was built with; no `dyn` (CLAUDE.md).
#[allow(async_fn_in_trait)]
pub trait RateProvider {
    async fn services(&self, provider: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError>;
    async fn quote(&self, provider: &ProviderKey, request: &RateRequest) -> Result<Vec<Rate>, ProviderError>;
}
```

| Kind | Key | Lives | Who writes it |
| --- | --- | --- | --- |
| Built-in | `builtin` | `hypha-shipping` (pure functions, no port call) | Hypha |
| **External app** | `app_<name>` | A record in `hypha-shipping` (callback URL, secret, timeout); the host's HTTP client calls it | Anyone, in any language: a TypeScript service, an app on Workers |
| **Spore** | `spore_<name>` | A crate compiled into the host (Shippo, EasyPost, DHL, a merchant's own) | Rust developers |

- **Dispatch without `dyn`:** the host's `RateProvider` implementation is a struct with a
  field per spore and a branch per kind (`app_*` → HTTP callback), so futures stay
  provably `Send` and Workers builds stay simple. When the spore registry (0002 §6)
  exists, it generates this struct; until then a host lists its spores by hand.
- **The rate callback (external apps)**, as built (`crates/hypha-host-axum/src/shipping.rs`):
  - `POST <callback_url>`, `content-type: application/json`, with headers
    `Hypha-Signature: t=<unix seconds>,v1=<hex HMAC-SHA256 of "<t>.<raw body>" under the
    secret>` (Stripe's scheme, reused by webhooks in 0016) and `Hypha-Request-Id` (the
    body's `id`). The app should check the signature and refuse a `t` more than a few
    minutes old. The callback URL is parsed with WHATWG rules and may not carry
    credentials, so the host checked is the host called. The host calls only public
    addresses: names are resolved and private, loopback, link-local and other internal
    addresses dropped at connect time (so re-pointing a name later doesn't help), and IP
    literals are checked before the call (`outbound.rs`; `HYPHA_ALLOW_PRIVATE_CALLBACKS=
    true` lifts it for development). A transport failure reaches the merchant only as
    `unreachable`; the details stay in the host's log, so an answer can't map a network.
  - Body (amounts are `Money`, `{"amount": <minor units>, "currency": "EUR"}`; addresses
    are the API's postal address shape):

    ```json
    {
      "id": "rreq_…",
      "sent_at": "2026-10-15T10:00:00Z",
      "request": {
        "currency": "EUR",
        "origin": { "last_name": "…", "line1": "…", "city": "Zagreb", "country": "HR", … } | null,
        "destination": { … },
        "items": [{ "variant": "var_…", "sku": "MUG-1" | null, "quantity": 2,
                    "unit_price": Money, "weight_grams": 350 | null }],
        "subtotal": Money
      }
    }
    ```
  - The app answers `2xx` with

    ```json
    { "rates": [{ "service": "express", "name": "Courier Express", "amount": Money,
                  "estimate": { "min_days": 1, "max_days": 2 } | absent,
                  "reference": "quote-42" | absent }] }
    ```

    within the provider's timeout (default 3000 ms, 100 to 10000) and in at most 256 KiB.
    A timeout, a refused connection, a non-2xx status (redirects aren't followed), a
    bigger answer, a body that doesn't parse, a negative amount or another currency are
    failures for that request (SH7): its options are left out with `provider_failed`, and
    the zone's backups appear if no regular option was offered.
  - `services` is `GET <callback_url>/services` with the same headers (body empty, so
    the signed string is `"<t>."`), answered with `{"services": [{"code", "name"}]}`;
    an app that registered a fixed `services` list isn't asked.
  - Secrets are generated by Hypha (`hypha_shs_` + 64 hex characters), shown once on
    create and rotate, stored sealed with `SecretBox` (the provider's key as context).
  - The protocol lives in the axum host for now. When a second host (Workers) needs it,
    it moves into an adapter crate behind an `HttpClient` port, like `hypha-store-s3`.
- **Spores** implement a `ShippingRates` trait of their own (same two methods, no key
  argument); the host's dispatcher calls them. They read their credentials from the
  secrets port (0002 §6).

## 6. Rates on a cart (SH7, SH9)

- `GET /storefront/carts/{id}/shipping-options` returns, per group, the options offered
  for the cart's shipping address, each with its price (built-in rates computed now,
  provider rates quoted now) and estimate, and the options that couldn't be offered with
  a reason (`provider_failed`, `weight_unknown`, `no_price_in_currency`,
  `service_not_quoted`, `backup_not_needed`). Provider calls for one cart run
  concurrently, each under its timeout. It, selecting and checkout are rate limited per
  cart (`Rule::ShippingQuotesCart`), since each may wait on rate apps; per cart rather
  than per address, because a server-rendered storefront sends every shopper's requests
  from one address. Creating carts is limited per client address
  (`Rule::CartCreateIp`), so new carts can't get around the per-cart limit; a
  server-rendered storefront passes shoppers' addresses in `X-Forwarded-For` and is
  listed in `HYPHA_TRUSTED_PROXIES`.
- `PUT /storefront/carts/{id}/shipping/{group}` `{"option": "shipopt_…"}` selects one.
  For a provider option the quote is stored on the cart (amount, reference, quoted at).
- **Priced carts** include the selected option: built-in prices are recomputed on every
  read (cheap, and they follow the cart's subtotal); a provider quote is used as stored,
  and the cart shows when it expires (`quote_expires_at`, 15 minutes after quoting).
- **Checkout** re-quotes an expired provider quote, after the cart's own checks (empty,
  email, unavailable lines), and once per checkout even when it retries after losing a
  race. If the new price differs, checkout refuses with `total_changed` (the client
  shows the new total and retries with it); if the option is no longer offered, for any
  reason (the zone gone, the provider failing), `shipping_option_unavailable` (409).
- **Backup options** (`is_backup`) are offered only when no regular option of the zone
  is, whether those are disabled, failing or missing, so a carrier's outage (or a
  merchant switching options off) doesn't stop sales. Otherwise they're listed as
  `backup_not_needed`, and selecting one says so (409). Built-in prices are computed
  first: when a regular built-in option is offered, providers only backups use aren't
  asked; otherwise every provider is asked at once, so a regular provider's timeout
  isn't followed by a second wait for the backups'.

## 7. Carts, tax and orders

- **Cart** gains `shipping_address: Option<Address>` and `shipping_groups:
  Vec<ShippingGroup>` (`{id, lines, selected: Option<SelectedOption>}`), rebuilt when
  lines change (one group of every line that requires shipping, for now).
- **The address's country must be the cart's country** (`address_country_mismatch`,
  422): the cart's country already picked its market and taxes (0003, T2). Changing a
  cart's country is its own change, later (Q5).
- **Tax (SH10):** `TaxableLine` gains `kind: Item | Shipping`; shipping is taxed at the
  option's tax class or the region's default rate, rounded like lines (T6). Tax engines
  (later spores) receive the kind and may apportion shipping across items as some EU
  states require.
- **Totals:** `subtotal` (items), `shipping_total`, `tax_total` (items and shipping),
  `total`. A cart with shippable lines and no selected option shows its total without
  shipping and `shipping_ready: false`.
- **Checkout** refuses a cart that needs shipping without an address
  (`shipping_address_required`) or without a selection in every group
  (`shipping_option_required`), both 409 on the cart.
- **Order** snapshots `shipping_address` and `shipping_lines[]` (`{group, option,
  code, name, provider, service, amount, tax, estimate, reference}`) and
  `shipping_total`; fulfilment (0018) reads them.

## 8. API and admin

| Route | Scope | Notes |
| --- | --- | --- |
| `POST/GET /admin/shipping-zones`, `GET/PATCH/DELETE …/{id}` | `shipping:write` / `:read` | A zone and its countries |
| `POST /admin/shipping-zones/{id}/options`, `PATCH/DELETE …/options/{option}`, `PUT …/options/order` | `shipping:write` | Options with their rates (a zone's `GET` lists them) |
| `POST/GET /admin/shipping-providers`, `PATCH/DELETE …/{key}` | `shipping:write` / `:read` | External app providers (callback URL, timeout); spores are listed read-only |
| `POST /admin/shipping-providers/{key}/secret` | `shipping:write` | Rotates the callback secret, shown once |
| `GET /admin/shipping-providers/{key}/services` | `shipping:write` | For the option editor; may call the app's URL, so not for readers |
| `PUT /storefront/carts/{id}/shipping-address` | publishable key | The `Address` shape customers use |
| `GET /storefront/carts/{id}/shipping-options` | publishable key | Per group, with prices and reasons |
| `PUT /storefront/carts/{id}/shipping/{group}` | publishable key | Selects an option |
| `PATCH /admin/products/{id}/variants/{variant}` | `products:write` | Gains `weight`, `requires_shipping` |
| `PATCH /admin/locations/{id}` | `inventory:write` | Gains `address` |

New scopes `shipping:read` and `shipping:write`. **Admin:** Settings → Shipping (zones
with their options, a rate editor per option kind, providers with "test this callback");
weight and "requires shipping" in the product page's variant table.

## 9. Later, and what's reserved for it

- **Shipping profiles** (product groups with their own options: oversized, cold chain):
  options gain an optional profile; groups split by profile. Additive, since a cart
  already has groups.
- **Local pickup and local delivery:** a group's method becomes `shipping | pickup`
  (UCP's `type`) with a location as destination. The `shipping_groups` field name is the
  only thing to revisit; Q3 keeps it neutral if you prefer.
- **Split shipments by location** (multi-origin): more groups per cart.
- **Provinces and postal codes** in zones; changing a cart's country.
- **Fulfilment providers** (labels, tracking): the same providers gain a second port in
  0018, and a quote's `reference` buys its label.
- **Rate apps without a held connection.** Offers, selecting and checkout call rate apps
  (up to 10 s each) while the request holds its pooled Postgres connection, so slow apps
  under load can starve the pool for every route. For now a per-cart rate limit
  bounds the storefront routes (§6); it doesn't bound a flood of new carts (cart
  creation has no limit yet either). The fix: read the cart, release the connection,
  call the apps, then take a connection again to write (re-checking the cart's version,
  as checkout's retry already does). It changes how hosts hand ports to these commands
  (two short borrows instead of one per request), so it's its own change.

## 10. Open decisions

**Q1. How providers plug in (SH5).** *Decided: A.*
- **A. Three kinds** behind one port: built-in rates, external apps through a signed rate
  callback, and Rust spores. TypeScript developers (0002's primary extenders) write
  carriers without Rust, as on Shopify; spores serve in-process needs.
- **B. Spores only:** simplest, but every custom carrier needs a Rust crate and a host
  build, against 0002 §1.
- **C. HTTP callbacks only:** even Hypha's own carrier integrations become external
  services; no in-process option for hosts that want one binary.

**Q2. What an option is to a provider.** *Decided: A.*
- **A. One service per option**, named by the merchant ("Express" = DHL Express): stable
  option IDs a buyer and UCP can select, and the merchant controls what's shown.
- **B. One option shows every rate the provider returns** (Shopify's carrier-calculated
  rates): no setup per service, but the choices and their IDs change per quote.

**Q3. The cart's shipping shape.** *Decided: A* (`shipping_groups`).
- **A. `shipping_groups: [{id, lines, options, selected}]`**, one group for now:
  mirrors UCP's groups; split shipments and profiles add groups without a breaking change.
- **B. One `shipping: {option, …}`** per cart: simpler now; splitting later is a new field
  and a breaking change for storefronts.
- **C. A, but named `delivery_groups`**, so pickup and local delivery fit the name too.

**Q4. Where shipping data about products lives (SH11).** *Decided: A, refined as
"facts on the product, policies in the domain that applies them".* Both options kept the
hexagon intact (shipping sees plain values, carriers stay behind a port); the question
was ownership between bounded contexts. Weight describes the product however it ships,
so it stays with the product; which options apply to which products is shipping's
policy, so profiles will live in `hypha-shipping`.
- **A. On the variant** (`weight`, `requires_shipping`), like Shopify: one place to edit,
  read with the product the cart already loads.
- **B. In `hypha-shipping`, linked by variant ID** (like tax classes): products stay
  unaware of shipping, but every cart reads one more table, and the admin edits two places.

**Q5. A shipping address in another country than the cart's.** *Decided: A.*
- **A. Refuse it** (`address_country_mismatch`); changing a cart's country (and maybe its
  market) is a separate step later.
- **B. Let the address move the cart's country**, re-picking market, currency and taxes:
  what buyers expect, but it reprices the whole cart and can change currency mid-checkout.

**Q6. Calculated rates between showing and paying (SH9).** *Decided: A.*
- **A. Keep the quote 15 minutes; re-quote at checkout and refuse a changed price** with
  `total_changed`.
- **B. Re-quote on every cart read:** always fresh, but every read waits on carriers.
- **C. Honour a quote until checkout, however old:** the store may undercharge.

**Q7. Tax on shipping (SH10).** *Decided: A.*
- **A. The option's tax class, or the region's default rate.** EU apportionment (shipping
  taxed in proportion to the goods' rates) is left to tax-engine spores.
- **B. Apportion now**, in the built-in tax: more correct for mixed-rate carts in the EU,
  but more rules to get right before anyone needs them.

**Q8. Built-in rate kinds for the first version (SH4).** *Decided: A.*
- **A. Flat, free over a subtotal, subtotal tiers, weight tiers.**
- **B. Flat and free-over only;** tiers later.

## 11. Commits (after the answers)

On `feat/shipping`:

1. `docs: design note 0015` (with the answers).
2. `feat(products)`: weight and `requires_shipping` on variants; `feat(inventory)`:
   location addresses.
3. `test(testkit)` contract scenarios, then `feat(shipping)`: zones, options, built-in
   rates, the `RateProvider` port, the callback provider record.
4. `feat(tax)`: shipping lines.
5. `feat(cart)`, `feat(checkout)`, `feat(orders)`: address, groups, quotes, totals,
   snapshots.
6. `feat(host-axum)`: routes, the HTTP rate callback client, the provider dispatcher,
   OpenAPI, e2e (with a test callback server).
7. `feat(admin)`: Settings → Shipping, variant weight.
