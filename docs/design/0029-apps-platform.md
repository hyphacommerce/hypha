# 0029: The apps platform: extension points, installation, and app UI in any admin or storefront

- **Status:** Accepted (2026-10-18). Replaces the spore design this note first held
  (compiled-in Rust extensions, 0002 §6): Danilo asked how Shopify lets apps change its
  behaviour, and whether that model fits a headless Hypha better. It does: this note
  makes apps the one public way to extend Hypha. Decided: **OAuth and staff-approved
  scopes from day one** (Q1), with every change of scopes approved again, and personal
  data behind its own scopes (AP10) so the merchant consents to exactly what an app sees;
  the other questions as recommended.
- **Scope:** what an app may do and how; extension points (synchronous contracts Hypha
  calls apps through) and the first ones (payments, search, and shipping rates moved
  onto the mechanism); the app manifest, installation and credentials; how an app's UI
  appears in an admin and a storefront, ours or anyone's, in any framework; app data on
  core objects; first-party apps deployed next to Hypha (Stripe, Meilisearch); what
  replaces spores.
- **Not in scope:** Functions (sandboxed wasm for hot-path rules, §11: named here as a
  later tier); an app marketplace and public OAuth (Q1); billing; the MCP surface (its
  own note, next: it reuses this note's scopes and app credentials).
- **Related:** 0002 (tiers: apps were already first; §3 custom fields), 0008 (API keys,
  scopes), 0010 (the two API surfaces, OpenAPI), 0015 (the shipping rate callback: the
  first extension point, SH5/SH6), 0016 §8 (`hypha-apps`, webhooks, signatures), 0020
  (sales channels), 0023 (the storefront), 0025/0026 (deployments, Service Bindings),
  0027 (payment providers), 0019 (search).

## Why

Hypha is headless: our admin and storefront are examples, not requirements. An
extension model that needs our admin to show its settings, or Rust compiled into the
binary to change a flow, doesn't fit. Shopify's model does: apps are external services
that use the same API as everyone, hear about events, are *called* by the platform at
defined moments, and contribute UI that whatever admin or storefront is in front of the
merchant can show. Hypha already has half of it (apps with signed webhooks, the shipping
rate callback, scoped API keys); this note designs the rest once, so Stripe and
Meilisearch are apps and third parties can build the same way in any language.

## Decisions

| | Decision |
| --- | --- |
| AP1 | **Apps are the one public extension model.** An app is an external service (any language, any host), untrusted by design: it acts through scoped credentials, is called through signed contracts, and its UI runs sandboxed or is rendered by the host. No compiled-in third-party code (spores are dropped; §12 keeps a custom-host escape hatch). *Hard to reverse (the platform's trust model)* |
| AP2 | **Five channels, no others**: the Admin API (and Storefront API) with the app's scopes; webhooks (exist); **extension points**, synchronous calls Hypha makes to an app mid-flow; **app endpoints** reached through Hypha (an app proxy, §8); **UI extensions** (§6–§7). Everything an app contributes is declared in its manifest |
| AP3 | **An extension point is a published, versioned contract**: a name (`payments.start`), a version, request and response types (Rust DTOs, documented in a third OpenAPI document, `openapi/extensions.json`), a timeout, a failure policy (fail closed, or fall back to the default), an idempotency key, and the existing request signature (0015 §5, 0016 §8). Hypha calls, logs every call (admin-visible), and never runs app code. *Hard to reverse (contracts apps are written against)* |
| AP4 | **The manifest** (`hypha-app.json`, served by the app at a URL or uploaded) declares identity, the scopes it asks for, webhook subscriptions, the extension points it implements (with their versions and URLs), its UI extensions, its proxy path and its settings schema. Hypha validates it against a published JSON Schema. *Hard to reverse (format)* |
| AP5 | **Installing an app is OAuth 2.1** (authorization code with PKCE): the app's **client ID is its manifest URL** (a client ID metadata document, as MCP's authorization uses), so any deployment can be authorized by any app with no registration step. A staff member approves the requested scopes on a consent screen; the app exchanges the code for an access token and a refresh token bound to the app and exactly the approved scopes. **Any change of scopes is a new authorization the staff approve**; until then the app keeps the old grant. Uninstalling revokes every token. *Decided (Q1); hard to reverse (the flow apps implement)* |
| AP10 | **Personal data has its own scopes.** `orders:read` or `customers:read` give an app the records without personal fields; names, emails, phone numbers and addresses need `customer_data:name`, `:email`, `:phone`, `:address` (Shopify's protected customer data, per field). Without them those fields are absent from API responses, webhook payloads and extension-point requests to that app, so the consent screen says precisely which personal data an app sees. Staff sessions are unaffected. *Decided (Q1's intent); hard to reverse (scope names, the redaction contract)* |
| AP6 | **UI extensions come in three forms, each usable by any frontend**, from most portable to most integrated: **data and actions** (rendered generically from schemas), **declarative blocks** (a small JSON UI tree the host renders with its own components), and **embedded pages** (the app's own page in an iframe with a postMessage bridge). Storefronts add **elements**: web components loaded from the app. Every frontend discovers them through the API. *Hard to reverse (slot names, the block format, the bridge protocol)* |
| AP7 | **App data lives on core objects as metafields** (0002 §3's custom fields, built for this): namespaced by app (`app_reviews.rating`), typed, with read and write access by surface, so a storefront shows an app's data without calling the app |
| AP8 | **First-party apps are apps**: Stripe (payments) and Meilisearch (search) are built against the same public contracts, in their own repositories, and `hypha deploy` can deploy them next to the API (a Worker reached through a Service Binding on Cloudflare, a container on a VPS) so an operator still runs one command and no request leaves the deployment unless the app itself calls out |
| AP9 | **Shipping rates move onto the mechanism** as the extension point `shipping.rates` v1 (today's callback is its v1 body, unchanged on the wire), so there is one way Hypha calls apps |

## 1. What others do

- **Shopify:** apps are external. They use the Admin API (OAuth install, scopes) and
  webhooks; Shopify calls them synchronously for **Payments Apps** (the app starts and
  resolves payment sessions), **Carrier Service** (rates) and **Fulfillment Service**;
  **Functions** run small sandboxed wasm rules in-process (discounts, payment and
  delivery customization, validation); **UI**: embedded apps are iframes with App Bridge
  (session tokens, navigation, toasts, resource pickers); **Admin and checkout UI
  extensions** run the app's script in a sandboxed worker that describes UI through
  `remote-dom`, which Shopify renders with its own components, so apps can't break the
  page and always look native; **theme app extensions** add blocks to themes; the **App
  Proxy** forwards `shop.com/apps/<x>/*` to the app with a signature; **metafields and
  metaobjects** hold app data on core objects.
- **Saleor:** moved from in-process plugins to apps; payments via synchronous webhooks
  (Transactions API); the dashboard mounts app iframes at declared points (`extensions`
  with a `mount` and `target`).
- **Medusa:** plugins in-process; admin widgets bundled at build time into named zones,
  which only works with Medusa's admin.
- **Headless lesson:** Shopify's custom storefronts (Hydrogen) can't run theme app
  extensions or checkout UI extensions outside Shopify's own surfaces; apps that want to
  reach headless storefronts expose data (metafields) and components (npm, web
  components). Hypha designs for headless first.

## 2. The five channels

| Channel | Direction | Authenticated by | For |
| --- | --- | --- | --- |
| Admin / Storefront API | app → Hypha | app access token (scopes) | reading and writing the store; reporting results (a payment captured) |
| Webhooks | Hypha → app, async | `Hypha-Signature` (app secret) | reacting to events (exists, 0016) |
| Extension points | Hypha → app, sync | `Hypha-Signature`, idempotency key | changing a flow (payments, rates, search) |
| App proxy | frontend → Hypha → app | the frontend's own credentials; Hypha signs the forwarded call with the shopper or staff context | app endpoints a storefront or admin calls without knowing the app's address |
| UI extensions | app → any frontend | session tokens (§6) | settings, order blocks, pages, checkout widgets |

Apps never talk to each other directly; they share events and metafields.

## 3. Extension points (AP3)

```rust
/// A synchronous contract Hypha calls apps through. One impl per point and version.
pub trait ExtensionPoint {
    const NAME: &'static str;      // "payments.start"
    const VERSION: u16;            // a reshaped request or response is a new version
    type Request: Serialize + ToSchema;
    type Response: DeserializeOwned + ToSchema;
    const TIMEOUT: Duration;
    const ON_FAILURE: OnFailure;   // FailClosed (the flow stops with a typed error) | Fallback
}
```

- **Calling:** core's `call::<P>(app, request, idempotency_key)` signs (`Hypha-Signature`
  over timestamp and body, the app's secret), sends with `Hypha-Extension`,
  `Hypha-Extension-Version`, `Idempotency-Key`, reads at most 256 KiB, validates the
  response against `P::Response` (a malformed answer is a failure, not a 500), and
  records the call (`extension_calls`: app, point, duration, outcome, error; pruned).
- **Registering:** the manifest lists `{ "point": "payments.start", "version": 1,
  "url": "https://…" }`; installing checks each against the points this Hypha knows.
  Several apps may implement a point; which one is used is per-object configuration
  (a payment method names its app, a shipping option its provider, the store its search
  app).
- **Transport:** through the host's outbound client (public addresses, signed, as
  today), or a **Service Binding** when the app is co-located on Cloudflare (§9): no
  public hop, no request fee.
- **Failure:** `FailClosed` points return a typed error to the caller
  (`app_unavailable`, with the app and point in `details`); `Fallback` points use core's
  default and log. Timeouts are short by default (rates 5 s, payments 15 s, search 3 s).
- **Versioning:** Hypha calls the newest version both sides support; an app declaring
  only an old version keeps working until that version is retired (announced, with the
  admin listing apps on deprecated versions).

### 3.1 The first points

| Point | Called when | Response | On failure |
| --- | --- | --- | --- |
| `shipping.rates` v1 | a cart's shipping options are priced | rates (today's body) | fall back: the app's options are hidden |
| `payments.start` v1 | checkout starts a payment with an app's method | next action: `redirect`, `client_widget { element, props }`, `instructions` | fail closed: `app_unavailable`, the order isn't placed |
| `payments.capture`, `.void`, `.refund` v1 | staff act on a payment | accepted / refused with a reason | fail closed |
| `search.query` v1 | `/storefront/search` with the store's search app set | product and variant IDs in order, facet counts, total | fall back to core's engine |

Payment results come back through the API: the app calls
`POST /admin/payments/{id}/events` (`authorized`, `paid`, `failed`, `voided`,
`refunded`, with the provider's reference and an idempotency key), which moves the
payment and its order in one batch (0027's rules). So a payment app's webhook from its
own provider (Stripe's) lands on the app, which reports to Hypha.

Later points, same mechanism: `tax.calculate`, `inventory.availability` (an ERP answering
stock for add-to-cart and checkout), `fulfillment.request`, `cart.validate`,
`discounts.calculate` (or Functions, §11).

## 4. Manifest and installation (AP4, AP5, AP10)

```json
{
  "$schema": "https://hypha.dev/schemas/app-manifest/1.json",
  "key": "app_stripe",
  "name": "Stripe",
  "version": "1.2.0",
  "app_url": "https://stripe-app.example.com",
  "redirect_uris": ["https://stripe-app.example.com/hypha/oauth/callback"],
  "scopes": ["payments:write", "orders:read", "customer_data:email"],
  "scope_reasons": { "customer_data:email": "Stripe sends the receipt to the buyer" },
  "webhooks": { "url": "/hypha/webhooks", "events": ["order.cancelled"] },
  "extension_points": [
    { "point": "payments.start", "version": 1, "url": "/hypha/payments/start" },
    { "point": "payments.capture", "version": 1, "url": "/hypha/payments/capture" }
  ],
  "settings": { "schema": { "...": "JSON Schema" }, "secret_fields": ["secret_key"] },
  "admin": {
    "pages": [{ "id": "dashboard", "title": "Stripe", "url": "/admin", "nav": "apps" }],
    "blocks": [{ "id": "payment", "target": "order.payment", "url": "/hypha/blocks/payment" }],
    "actions": [{ "id": "capture", "target": "payment", "label": "Capture",
                  "point": "payments.capture" }]
  },
  "storefront": {
    "elements": [{ "id": "payment-element", "target": "checkout.payment",
                   "module": "/elements/payment.js", "tag": "stripe-payment" }],
    "proxy": "/hypha/storefront"
  }
}
```

### 4.1 The OAuth flow (AP5)

Hypha's API is the authorization server; the consent screen is a page of whichever
admin the store uses (`server.admin_url`), so it stays headless:

1. **Start:** the app (from its own site, or after a staff member pastes the app's URL
   into Settings → Apps) sends the browser to
   `GET {api}/oauth/authorize?client_id=<manifest URL>&redirect_uri=…&scope=…&state=…&code_challenge=…&code_challenge_method=S256`.
2. **Checks:** Hypha fetches the manifest (the client metadata), checks the redirect URI
   is one it lists exactly, that the requested scopes are ones it declares, and stores
   a pending **authorization request** (10 minutes). It redirects to the admin's consent
   page, `{admin}/apps/consent?request=<id>`.
3. **Consent:** the admin reads `GET /admin/oauth/requests/{id}` (app name, publisher
   URL, icon, scopes grouped by area, **personal-data scopes listed separately with the
   manifest's reasons**, what's new compared with the current grant). A staff member
   needs `apps:write` and must hold every scope requested (as role grants require,
   0008). `POST /admin/oauth/requests/{id}/approve` or `/deny`; Hypha redirects to the
   app's redirect URI with `code` and `state` (or `error=access_denied`).
4. **Token:** the app calls `POST {api}/oauth/token` (`grant_type=authorization_code`,
   the code, the PKCE verifier, `client_id`); on first install the response includes
   the **app secret** (for verifying Hypha's webhook and extension-point signatures),
   once. Access tokens (`hypha_at_…`) live an hour; refresh tokens (`hypha_rt_…`)
   rotate on use, and a reused one revokes the grant (theft detection).
5. **Registrations** (webhooks, extension points, UI extensions) take effect when the
   grant does, filtered by it: a webhook for an event whose payload needs a scope the
   grant lacks isn't delivered; an extension point needing a scope isn't called.

- **Scope changes:** a manifest that now asks for more (or the app requesting a new
  `scope` at `/oauth/authorize`) starts a new request showing exactly what changes; the
  old grant stays in force until it's approved; fewer scopes take effect at once. Staff
  can also narrow a grant from the admin (`PATCH /admin/apps/{key}/grant`), which the
  app learns by webhook (`app.grant_changed`).
- **Grants are visible:** `GET /admin/apps/{key}` shows the grant (scopes, who approved,
  when, the request history); every token use is attributed to `Actor::App`, so writes
  are stamped with the app in the outbox and audit trail.
- **Uninstall:** `DELETE /admin/apps/{key}` revokes all tokens, removes registrations
  and sends `app.uninstalled` (the last webhook it gets).
- **Private apps** (a store's own integration with no public site) use the same flow
  with a manifest uploaded to the admin; today's manually created apps (0016) become
  private apps once a staff member approves their scopes (until then they keep
  webhooks only).

### 4.2 Personal data (AP10)

| Scope | Unlocks |
| --- | --- |
| `customer_data:name` | first and last names (customers, addresses on orders, carts) |
| `customer_data:email` | email addresses |
| `customer_data:phone` | phone numbers |
| `customer_data:address` | street lines, postal code, city, province (country and company stay visible) |

- Redaction is applied where data leaves for an app: response DTOs (the field is
  absent and the object says `"redacted": ["email", …]`), webhook envelopes, extension
  point requests, the proxy's context headers. One function decides it per field, so
  every channel agrees; a contract test runs each route with an app token without the
  scopes and checks no personal field leaks.
- The same scopes apply to API keys and agents (anything that isn't a staff session),
  so an integration key made for a dashboard doesn't see emails by default.
- Data an app already received isn't Hypha's to delete; uninstall tells the app, and the
  consent screen links to the app's privacy policy (a required manifest field).
- **Settings:** an app keeps its own settings (its database). Hypha stores none, except
  what a merchant must not hand an app's UI: none in v1. The manifest's settings schema
  is for frontends that render a settings form without the app's page (§6.1).
- **Updates:** re-fetching the manifest (`POST /admin/apps/{key}/refresh`, or the app
  calls it) applies new registrations; new scopes go back to `pending` approval.
- **Private apps** (a store's own integration) skip the URL: the manifest is uploaded.
  Today's manually created apps (0016) are private apps with no manifest.

## 5. Where app UI can appear: targets

UI extensions name a **target**, a stable slot name that any frontend may implement.
The catalog (versioned, in `openapi/extensions.json`):

| Target | Surface | Context passed |
| --- | --- | --- |
| `apps.page` | admin: a full page under the app | none |
| `settings.app` | admin: the app's settings | none |
| `order.details`, `order.payment`, `order.fulfillment` | admin: blocks on an order | `order_id` (and `payment_id`, …) |
| `product.details`, `customer.details` | admin: blocks | the object's ID |
| `payment` (actions) | admin: buttons on a payment | `payment_id` |
| `checkout.payment` | storefront: the payment step | `order_id`/`cart_id`, the next action's props |
| `product.block`, `cart.block`, `order.confirmation` | storefront: blocks | the object's ID |

A frontend shows what it supports and ignores the rest: our admin will implement the
admin targets; a custom admin may implement none, some or all.

## 6. Admin UI extensions, in any admin

The challenge: a custom admin (any framework) must be able to show what an app
contributes without knowing the app. So everything is **discoverable and rendered from
data**: `GET /admin/app-extensions?target=order.payment&context=…` lists each installed
app's contributions for a target, in one of three forms.

### 6.1 Data and actions (no app UI at all; works everywhere)

- **Actions:** `{ app, id, label, target, input_schema? }`. A frontend shows a button
  (and a form generated from `input_schema` when there is one); pressing it is
  `POST /admin/app-extensions/actions/{app}/{id}` with the context, and Hypha calls the
  app (an extension point or the app's action URL), returning `{ message, refresh }`.
- **Settings form:** the manifest's JSON Schema, rendered by any JSON-Schema form
  library; saved through the app proxy to the app.
- **Metafields:** displayed by definition (type, label), editable where the definition
  allows staff writes.

Even a command-line admin or an agent can use this form (MCP exposes actions as tools).

### 6.2 Declarative blocks (the host renders; any framework)

The app's block URL answers, through the proxy, a **UI tree** in a small, closed,
versioned vocabulary, rendered by the host with its own components, so blocks look
native in every admin and can't run code in it:

```json
{ "version": 1, "root": {
  "type": "card", "title": "Stripe",
  "children": [
    { "type": "row", "children": [
      { "type": "text", "value": "Captured" },
      { "type": "badge", "value": "succeeded", "tone": "success" } ] },
    { "type": "link", "label": "Open in Stripe", "href": "https://dashboard.stripe.com/…",
      "external": true },
    { "type": "button", "label": "Refund", "action": "refund" } ] } }
```

- Vocabulary v1 (about fifteen elements): `card`, `stack`, `row`, `text`, `heading`,
  `badge`, `link`, `button`, `divider`, `image`, `list`, `table`, `keyvalue`, `form`
  with `field`s (text, number, select, checkbox), `banner`. Buttons name actions (6.1)
  or `navigate` to an app page.
- Hypha publishes the JSON Schema and **reference renderers** as packages: React
  (Astryx, used by our admin) first; the format is plain JSON, so a Vue or Svelte admin
  writes its own renderer in an afternoon.
- Like Slack's Block Kit or Shopify's admin UI extensions, minus running app scripts in
  the host: v1 blocks are fetched, not computed in the browser (Q2).

### 6.3 Embedded pages (the app's own UI; any framework on both sides)

- The app's page (`apps.page`, `settings.app`, or a block that needs real UI) loads in
  an `<iframe sandbox="allow-scripts allow-forms allow-same-origin allow-popups">`
  pointing at the app's URL.
- **The bridge** is a postMessage protocol, published with two tiny packages:
  `@hypha/app-bridge` (inside the app: `getSessionToken()`, `navigate()`,
  `toast()`, `openResourcePicker()`, `resize()`, `close()`) and
  `@hypha/app-bridge-host` (in any admin: one function that answers those messages
  for an iframe). Framework-free on both sides; the protocol itself is documented so
  either side can be written without our packages.
- **Session tokens:** the host asks the API for a short-lived token
  (`POST /admin/app-extensions/{app}/session`: a JWT, 1 minute, audience the app, with
  the staff member, the store and the app's scopes for that staff member), and passes it
  through the bridge; the app verifies it (signed with the app's secret, Q4) and calls
  its own backend with it. The app never sees the staff session.
- The app sets `Content-Security-Policy: frame-ancestors` to the admins it trusts;
  Hypha lists the store's admin origins (`GET /admin/app-extensions/origins`, from
  configuration) so an app can do that per store.

## 7. Storefront UI, in any storefront

Storefronts are the merchant's code, so apps provide **data, endpoints and
framework-free components**, and the storefront decides placement:

- **Data:** metafields with storefront visibility appear in Storefront API responses
  (`product.metafields`), so a reviews app's rating renders anywhere with no app call.
- **Endpoints:** the app proxy (§8) under `/storefront/apps/{app}/…`, called with the
  storefront's publishable key; Hypha forwards with the shopper's context signed.
- **Elements:** web components (custom elements) the app hosts as ES modules:
  `GET /storefront/app-extensions?target=checkout.payment` lists `{ app, tag, module,
  props }`; the storefront loads the module and renders `<stripe-payment …>`. Custom
  elements work in React, Vue, Svelte or plain HTML; Hypha publishes
  `@hypha/storefront-elements` (a loader and typed React wrappers) for convenience.
  Elements talk to Hypha only through documented events
  (`hypha:payment-confirmed`, `hypha:error`) and the app's proxy.
- **Payments specifically:** `payments.start` answers `client_widget { element, props }`
  (e.g. Stripe's Payment Element with the client secret); the storefront mounts that
  element in the payment step and waits for `hypha:payment-confirmed`, then shows the
  confirmation (the payment's final state arrives by the app reporting to Hypha). A
  storefront that won't run app code can use `redirect` instead, if the app offers it.
- **Trust:** an element runs in the storefront's page with the page's privileges: the
  merchant trusts it by installing the app, as with any script tag. Elements in a
  sandboxed iframe are the stricter option (Q3).

## 8. The app proxy

`/admin/apps/{app}/proxy/{path}` and `/storefront/apps/{app}/proxy/{path}`: Hypha
authenticates the caller (staff session or key; publishable key and shopper context),
forwards to the manifest's proxy URL with `Hypha-Signature` and context headers
(`Hypha-Store`, `Hypha-Staff` or `Hypha-Customer`/`Hypha-Cart`, `Hypha-Channel`,
`Hypha-Country`), and returns the app's answer (size and time capped). It gives frontends
one origin and one credential for every app, keeps app addresses private, and gives
apps verified context without their own auth. Blocks (6.2) and settings saves (6.1) go
through it.

## 9. First-party apps, deployed alongside (AP8)

- **Repositories:** `hypha-app-stripe`, `hypha-app-meilisearch`, written in TypeScript
  on our app SDK (`@hypha/app`: manifest types, signature checks, extension-point
  handlers, bridge), each deployable alone like any third-party app.
- **`hypha deploy` co-locates them** when `hypha.json` lists them
  (`"apps": ["stripe"]`): on Cloudflare, each as its own Worker with a Service Binding
  from the API (so extension-point calls never leave Cloudflare's network and cost no
  request fee) and from the admin and storefront for their proxies; on a VPS, as
  containers in the compose file. First-party apps get no shortcut: the deploy starts their OAuth authorization and prints (and opens) the consent link, and a staff member approves their scopes like any app's (§4.1).
- **Stripe app:** its own D1 or KV for its settings and Stripe's webhook dedupe;
  receives Stripe's webhooks itself (its own public URL) and reports to Hypha
  (`POST /admin/payments/{id}/events`); `payments.start` creates PaymentIntents
  (idempotency key = the payment's ID); admin block on `order.payment`, settings page,
  a `stripe-payment` storefront element.
- **Meilisearch app:** keeps per-market indexes from webhooks (product, price,
  inventory, catalog events) and Admin API reads; implements `search.query`; settings
  page (Meilisearch URL and key). The storefront keeps calling `/storefront/search`.

## 10. What core builds

1. The extension-point mechanism (§3) and `extension_calls`; `shipping.rates` moved onto
   it (AP9); `openapi/extensions.json`.
2. Manifest, install, approve, refresh, uninstall; app access tokens with `Actor::App`.
3. The proxy (§8).
4. `GET /admin/app-extensions` and `/storefront/app-extensions` (discovery), actions,
   session tokens; the target catalog.
5. Payments: `payments.start/capture/void/refund` as points, payment methods naming an
   app (`app_<key>` provider), `POST /admin/payments/{id}/events`, the storefront retry
   route (0027 §4).
6. Search: the store's search app setting and `search.query` with fallback.
7. Metafields (custom fields, 0002 §3) as their own note, needed by AP7.
8. Our admin and storefront implement the targets: the bridge host, the block renderer,
   the element loader, Settings → Apps with installation.
9. `@hypha/app` (SDK) and `@hypha/app-bridge`, `@hypha/app-bridge-host`,
   `@hypha/storefront-elements`, the block renderer package.

## 11. Later: Functions

Small, pure rules on hot paths (discounts, hiding a payment or delivery method, cart
validation) can't afford a network call per cart change. Shopify's answer is
**Functions**: the app uploads a WebAssembly module with a typed JSON-in, JSON-out
contract; the platform runs it in a sandbox (no network, no clock, strict limits) at the
extension point, falling back if it fails. Hypha would add them as another way to
implement an extension point (wasmtime natively; Cloudflare's Worker Loader or an
interpreter inside the Worker), when discounts arrive.

## 12. What replaces spores

Spores (compiled-in Rust) are dropped as a public mechanism. What they would have given,
and its replacement:

| Spores gave | Instead |
| --- | --- |
| Code inside Hypha's transaction | Extension points before the write (validation), metafields written by core |
| No network hop | Functions (§11); co-located apps over Service Bindings |
| Nothing extra to host | `hypha deploy` deploys first-party apps alongside |
| Swapping infrastructure (search engine, email sender) | Core's own engines chosen in configuration (as senders already are, 0028) |

**The escape hatch:** Hypha's hosts are thin binaries over library crates, so a team
that truly needs in-process code writes its own host crate depending on Hypha's crates.
That's documented, not a framework. The `spore_` prefixes reserved in keys (shipping,
payments, media usage) stay unused; a later cleanup may remove them.

## 13. Open decisions

**Q1. Install flow.** *Decided:* OAuth from day one (§4.1), staff approve every grant
and every change of scopes, and personal data sits behind its own scopes (§4.2).

**Q2. Declarative blocks: fetched JSON (v1) or app scripts rendering remotely?**
Shopify runs the app's script in a sandboxed Web Worker that describes UI (remote-dom),
so blocks can be interactive without round trips. Recommended: fetched JSON trees in v1
(simpler for every host to implement, any language can produce them); a scripted mode
only if real apps need it.

**Q3. Storefront elements: in the page or sandboxed?** In the page (web components) is
what payment widgets need (Stripe Elements mounts its own iframes inside) and what
headless storefronts can use easily. Recommended: in the page, trusted at install,
with the element's origin listed so a storefront's CSP allows exactly the installed
apps.

**Q4. Session tokens signed with the app's secret (HS256) or Hypha's key (asymmetric,
JWKS)?** Recommended: the app's secret, since every app already holds one and verifies
webhooks with it; JWKS later if third-party verification without the secret is needed.

**Q5. Where first-party apps keep state.** Recommended: their own storage (D1/KV on
Cloudflare, a small Postgres schema or SQLite on a VPS), like any third-party app; not
Hypha's database, so they stay real apps.

**Q6. Our own admin and storefront: do they use these extension paths for first-party
features?** Recommended: no; core features stay core. But our admin implements every
admin target, as the reference for custom admins.

## 14. Build order

1. Extension points (§3) with `shipping.rates` moved onto them; the call log; the
   extensions OpenAPI document.
2. OAuth (authorize, consent requests, token, refresh rotation, revocation), the
   manifest as client metadata, grants and scope changes, the personal-data scopes with
   redaction across every channel (and its contract test); the proxy.
3. Discovery, actions, session tokens; the block format and the React renderer; the
   bridge packages; Settings → Apps in our admin with install and app pages.
4. Payments as extension points, payment events, the storefront element loader; the
   Stripe app (its own repository) end to end on both deployments.
5. `search.query` and the Meilisearch app.
6. Metafields (their own note).

## 15. Implementation contract

What the parts built in parallel agree on; where this differs from the sections above,
this wins.

**Crates.**
- `hypha-extensions` (leaf, new): the `ExtensionPoint` trait, `OnFailure`, the catalog
  of points (`shipping.rates` v1, `payments.start`/`capture`/`void`/`refund` v1,
  `search.query` v1) with their request/response DTOs, the `ExtensionCall` record and
  its repo (`extension_calls`), the `ExtensionCaller` port the host implements (sign,
  send over the outbound client or a binding, cap and parse), and `call::<P>` that
  validates, records and applies the failure policy.
- `hypha-apps` (leaf, extended): the manifest (`AppManifest`, parsed and validated from
  JSON, its published JSON Schema), registrations derived from it, the grant (scopes,
  approved by, at), authorization requests.
- `hypha-oauth` (composing, new; depends on `hypha-apps`, `hypha-auth`, core): authorize,
  approve/deny, token (authorization code + PKCE S256, refresh with rotation and reuse
  detection), revoke, grant changes. Tokens are `hypha-auth` tokens (stored as SHA-256)
  of new kinds: access `hypha_at_` (1 h), refresh `hypha_rt_` (30 days, rotating),
  authorization code `hypha_ac_` (60 s, single use).
- Redaction (AP10) lives in `hypha-http` (`redact.rs`): one function per DTO family
  over the caller's scopes, used by routes, webhook envelopes (dispatch asks the app's
  grant), extension requests and proxy headers.

**Actors and scopes.** `Actor::App(AppKey)` for app tokens. New scopes
`customer_data:name`, `customer_data:email`, `customer_data:phone`,
`customer_data:address`; staff sessions (`Actor::User`) are never redacted; every other
actor (keys, apps, agents) is.

**Tables.** `0049_extension_calls.sql` (`extension_calls`: id, app, point, version,
started_at, duration_ms, outcome `ok|failed|timeout|invalid`, status code, error,
correlation (order/cart/payment id), pruned like deliveries); `0050_oauth.sql`
(`app_grants`, `oauth_requests`, `oauth_codes`, `oauth_refresh_tokens` with a family id
for reuse detection; `apps` gains `manifest_url`, `manifest jsonb`, `manifest_fetched_at`,
`publisher`, `privacy_url`).

**Wire formats.**
- Extension calls: `POST <url>`, JSON body = the point's request, headers
  `Hypha-Signature` (as 0015 §5: `t=<unix>,v1=<hex hmac-sha256 of "<t>.<body>">` with
  the app secret), `Hypha-Extension: payments.start`, `Hypha-Extension-Version: 1`,
  `Idempotency-Key`, `Hypha-Store`. A 2xx with a body matching the response schema is
  success.
- OAuth endpoints: `GET /oauth/authorize`, `POST /oauth/token` (form-encoded, RFC 6749
  errors `{error, error_description}`), `POST /oauth/revoke`; discovery at
  `/.well-known/oauth-authorization-server` (RFC 8414). The consent page URL is
  `{server.admin_url}/apps/consent?request=<id>`.
- Admin routes: `GET /admin/oauth/requests/{id}`, `POST …/approve`, `POST …/deny`;
  `GET /admin/apps/{key}` gains `grant`, `manifest`, `registrations`;
  `PATCH /admin/apps/{key}/grant` (narrow); `DELETE /admin/apps/{key}` (uninstall);
  `GET /admin/apps/{key}/extension-calls`.
- Error codes: `app_unavailable` (502, `details: {app, point}`), `app_not_authorized`
  (403: an app's token lacks a scope), `oauth_request_not_found` (404),
  `oauth_request_expired` (409), `manifest_unreachable` / `manifest_invalid` (422 with
  `details.errors`).

**Packages** (pnpm workspace): `packages/app` (`@hypha/app`: manifest types, signature
verification, OAuth client with PKCE, extension handler types generated from
`openapi/extensions.json`, session-token verification), `packages/app-bridge` and
`packages/app-bridge-host` (the postMessage protocol, documented in
`docs/apps/bridge.md`), `packages/app-blocks` (the block JSON Schema, version 1, and a
React renderer on Astryx), `packages/storefront-elements` (the element loader and
events, `docs/apps/elements.md`).

## As built: payments and search through apps (§10 items 5–6)

- **Providers:** a payment method's provider is `manual` or an app's key (`app_<name>`;
  `spore_` is gone from `ProviderKey`). `PaymentProviders` grew `available` (async),
  `act` (capture, void, refund) and `start` answering `Started { next_action, reference }`;
  `hypha-http`'s `HostPayments` answers app keys through `call::<PaymentsStart>` and
  friends and delegates the rest to the host's built-in tuple. An app method is offered
  when the app is installed, enabled and registers `payments.start`; it takes no
  settings. `NextAction` is `instructions | client_widget { element, props } | redirect`
  (`client_secret` is dropped: nothing could produce it).
- **Checkout keeps the order on a failed start** (0027 §4, not this note's "the order
  isn't placed"): the payment stays `pending`, the answer has `next_action: null` and
  `payment_error` (the error body, `app_unavailable` with `{app, point}`), and
  `POST /storefront/orders/{id}/payment?token=` starts it again: a `pending` payment with
  the same idempotency key (its ID), after a `failed` one a new payment with the same
  method. Checkout takes an optional `return_url`, passed to the app. The provider's
  `reference` is kept on the payment. `customer_email` is sent only to an app whose
  grant has `customer_data:email` (AP10; never to an app made by hand).
- **Reports:** `POST /admin/payments/{id}/events` (`payments:write`, `Idempotency-Key`)
  is `hypha_order_actions::record_payment_event`: `authorized | paid | failed | voided |
  partially_refunded | refunded`, with `amount` (a refund's own amount; the whole
  payment otherwise), `reference`, `occurred_at`. New events `payment.authorized`,
  `.failed`, `.refunded`. A repeated report changes nothing. An app calling with its own token
  (`Actor::App(AppId)`) may report only on payments whose provider is that app (the
  provider key's app, loaded by key, has the caller's ID; `payment_not_yours`, 403,
  otherwise, a `manual` payment included); staff and API keys with `payments:write` on
  any (`check_reporter` in `payment_events.rs`).
- **Staff actions:** `POST /admin/payments/{id}/capture | void | refund` (`orders:write`)
  answer **202** when the app accepts and move nothing (Shopify's model: the app reports
  the result); `409 payment_refused` with `details.reason`; `409
  payment_action_unsupported` for `manual` or an app without that point. Captures and
  voids are whole; the idempotency key sent to the app is the request's
  `Idempotency-Key`, else `<payment>:capture` / `:void`.
- **Search:** `SearchSettings::search_app` (`PATCH /admin/search/settings`, checked
  through core's `AppPoints` port: `search_app_unavailable`). `/storefront/search` then
  uses `AppSearch`, a `SearchIndex` that asks the app and builds Hypha's own documents
  for its hits (`hypha_search::documents_for`, so prices, stock and channel visibility
  are Hypha's), with no `price` range; on failure the built-in engine answers.
- **Registrations:** `hypha_apps::find_endpoint` is the one rule, read through
  `hypha-http`'s `HostAppEndpoints` (`app_endpoints.rs`, also core's `AppPoints`, so
  the payment-method and search-app checks agree with the calls). An app installed
  through OAuth answers where its manifest's `extension_points` say (URLs resolved
  against `app_url`; the entry's version must be the one this build calls), and only
  while it is enabled, not uninstalled, and its grant has the point's scope (§4.1 step
  5). An app made by hand (no manifest) answers where staff set with
  `PUT /admin/apps/{key}/extension-points` (table `app_extension_endpoints`, migration
  0051 after OAuth's 0050); that route refuses an app with a manifest
  (`endpoints_from_manifest`, 409), and `GET` lists the manifest's registrations for
  one. Uninstalled: none.
- **Scopes per point** (`ExtensionPoint::SCOPE`, what the grant must have; an app made
  by hand has no grant and is called on what staff registered):

  | Point | Scope | Why |
  | --- | --- | --- |
  | `payments.start`, `.capture`, `.void`, `.refund` | `payments:write` | the app reports results through `POST /admin/payments/{id}/events`, which takes it: without it a payment app can't do its job |
  | `search.query` | `products:read` | it answers with products and variants |
  | `shipping.rates` | `shipping:read` | it answers for shipping's rate settings |

  Rate apps keep their URL in shipping's `AppProvider` (0015), not the manifest; the
  gate (`App::may_answer`) applies to them all the same (`shipping_provider_disabled` otherwise).
- **Personal data in requests (AP10):** `Redaction::for_app` (`redact.rs`) decides,
  as for webhooks: the app's grant, nothing for an app without one. `payments.start`'s
  `customer_email` needs `customer_data:email`. `shipping.rates`' destination follows
  the address rule exactly: without `customer_data:address` the street lines, postal
  code, city and province go, without `:name` the names, without `:phone` the phone;
  country and company stay, and `redacted` lists what went (`last_name`, `line1`,
  `city` are then absent, the optional fields `null`, so an unredacted body is byte for
  byte 0015's). So **a rate app that quotes by postal code or city must be installed
  through OAuth with `customer_data:address`**; an app made by hand (0016) gets the
  country only, which is enough for zone-style flat rates. The origin (the store's
  location) isn't customer data and is sent whole.
- **Actors:** writes by an app's token are stamped `Actor::App(AppId)` in the outbox
  and idempotency records like any actor (`AppId` is `Copy`).
