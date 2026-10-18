# 0029: Spores: compiled-in extensions, with Stripe and Meilisearch as the first two

- **Status:** Accepted (2026-10-18). Danilo set the frame: a spore API with Stripe and
  Meilisearch as reference spores, installation compiled in and chosen in `hypha.json`,
  admin settings as schema-driven forms. Decided: Q1 (Stripe's keys are the merchant's,
  sealed in the database), Q2 (**every spore in its own repository from day one**, which
  reshapes SP1, SP3 and SP4 below), Q3 (no spore routes in v1), Meilisearch first; the
  rest as recommended.
- **Scope:** what a spore is and what it may contribute; how one is written (the
  `hypha-spore` crate), installed (from its own repository, pinned in `hypha.json`) and composed into
  both hosts; its settings, secrets, HTTP, storage, scopes, events, webhooks and admin
  UI; the line between spores and apps; the two reference spores and the core changes
  each needs (payment webhooks and capture for Stripe, a configurable search engine for
  Meilisearch); the build order.
- **Not in scope:** loadable plugins (WASM components, Worker Loader) and merchant logic
  on the hot path (Shopify-Functions-style wasm), §10; custom fields and policies (0002
  §3–4, their own notes); third-party spore distribution (Q2); refunds (they come with
  Stripe's capture, but the order-side flow is 0018's follow-up).
- **Related:** 0002 §6 (the first `Spore` sketch: this note replaces it), 0016 (events,
  handlers; §10 asks for open scopes and a spore slot in batches), 0019 (search engines;
  SE2 names Meilisearch, §5.1 handlers), 0027 (payment providers; PA5 the tuple), 0028
  (senders), 0015 (shipping provider kinds), 0017 (configuration), 0025/0026 (builds on
  a VPS and Workers), 0011 (which assumed `spores/*` in the workspace; Q2 moved spores out).

## Why

Every seam a spore needs exists in pieces: the host's event-handler tuple, the payment
provider tuple, the `SearchIndex` port, `HostMail`, shipping's `spore_` keys, per-module
migrations. But each is filled by editing core: a host file, a `hypha-config` enum, a
match in `hypha-http`. Nothing says what a spore *is*, so the first one (Stripe, which
blocks card payments) would be wired by hand across a dozen files, and the second would
copy that. This note fixes the shape once, before Stripe and Meilisearch are written,
so they are "a repository, a line in `hypha.json`".

## Decisions

| | Decision |
| --- | --- |
| SP1 | **A spore is a Rust crate in its own repository (package `hypha-spore-<name>`, key `spore_<name>`), compiled into both hosts.** It depends on Hypha's crates by git; a build maps those to the Hypha checkout being built (§4), so one copy of each type exists. No runtime loading, no registration magic (`inventory`/`linkme` don't hold on Workers). Spores are trusted code: they run in-process, inside requests and transactions. *Decided (Q2); hard to reverse (the trust model)* |
| SP2 | **One descriptor per spore**: the crate's `Spore` value (trait in the new `hypha-spore` crate) names it, declares its settings (operator and merchant), scopes, events, migrations, and builds the parts it contributes. The contributions are typed into existing seams (`PaymentProvider`, `SearchIndex`, `EventHandler`, job steps, `ShippingRates`, `EmailSender`), never `dyn` collections: a host composes them with static dispatch, like `handlers.rs` today. *Hard to reverse (the API spores are written against)* |
| SP3 | **Hosts compose spores in one generated crate, `hypha-spores`**: its list is a `spores!` macro call (macro in `hypha-spore`) naming each spore once with the seams it fills, expanding to the hosts' handler, payment, engine, sender and job types. The committed crate has the empty list (today's sets exactly); builds generate it (§4). Both hosts depend on it, ending today's "keep the two in step by hand" |
| SP4 | **Installation is `hypha.json`'s `"spores": [{ "name": "stripe", "git": "git@github.com:invocation97/hypha-spore-stripe.git", "rev": "<sha>" }]`**: the CLI clones each spore at its pinned revision on the operator's machine (their git credentials, so private repositories work), stages a build copy with the generated `hypha-spores`, builds both binaries from it, asks for operator settings and secrets on `init`, writes `[spores.<name>]` and `.env`, and passes them on deploy. A deployment's spores (and their revisions) are visible at `GET /admin/spores` |
| SP5 | **Two settings tiers.** Operator settings: `[spores.<name>]` in `hypha.config.toml`, secrets as `HYPHA_SPORE_<NAME>_<KEY>` environment variables (what the deployment needs to start). Merchant settings: in the database, one document per spore (`spore_settings`), edited in the admin through a form generated from the settings type's JSON Schema (`schemars`); fields marked secret are sealed with `SecretBox`, write-only, never returned. A payment method's own `settings` (0027) stay per method. *Hard to reverse (where secrets live)* |
| SP6 | **Spores add no arbitrary routes.** Core owns one inbound route per spore, `POST /hooks/spore_<name>/{endpoint}`, unauthenticated, rate-limited: it hands the raw body and headers to the spore's `verify`, records the verified delivery in a core inbox (deduplicated by the provider's event ID) and answers 200; the spore's `handle` runs from the inbox like an event handler, with retries. Everything else a spore needs from the API (a payment's next action, a search) goes through core routes and its ports. *Hard to reverse (route surface)* |
| SP7 | **Outbound HTTP through one core port**, `hypha_core::http::HttpClient` (method, URL, headers, body → status, headers, body), provided by each host for operator-configured services (no public-address rule: these aren't merchant URLs). `hypha-mail-resend`'s and `hypha-store-s3`'s local ports fold into it |
| SP8 | **Scopes become open**: `Scope` gains a spore case written `spore_<name>:<action>` (`spore_stripe:read`), declared by the spore; a stored scope this binary doesn't know (a spore removed) is ignored when a role loads, not an error. The owner role holds every declared scope. *Hard to reverse (stored scopes, the API's scope strings)* |
| SP9 | **Storage**: a spore that needs tables ships a `MigrationSet` (module `spore_<name>`, tables `spore_<name>_*`, no foreign keys into core), run by the native `setup` like core's; its repositories use a **spore storage kit** that `hypha-store-postgres` exports (the request's `Queries`, column helpers, error mapping, a boxed `SporeWrite` slot in `PgBatch` so its rows commit with core aggregates). Built when the first spore needs a table; neither reference spore does (§7, §8) |
| SP10 | **Admin UI: schema forms first.** The admin lists installed spores under Settings → Spores, each with its merchant settings form (from SP5's schema) and status; a payment method of a spore's provider shows that provider's per-method form. Custom spore UI (zones, pages) waits for a spore that needs it. The storefront gets a spore's client parts as an npm package in its repository (`storefront/`), installed by the staged build |
| SP11 | **Spores and apps stay separate tiers** (0002 §1): apps are external, any language, untrusted, reached over signed webhooks and calls; spores are trusted Rust inside the process, for what must run in a request or transaction or needs a port (payments, search engines, tax, senders). A capability may get both (an app payment provider over synchronous webhooks, like Saleor's, later) |

## 1. What others do

- **Medusa v2:** a plugin is an npm package bundling modules, *module providers* (an
  implementation behind a core module's port: payment, notification, file), API routes,
  subscribers, jobs and admin widgets, installed in `medusa-config.ts` with options.
  Payment webhooks hit a core route, `/hooks/payment/{provider}`, which only emits an
  event with the raw body; a subscriber asks the provider's `getWebhookActionAndData`
  what it means (Stripe's verifies the signature, maps `payment_intent.succeeded` to
  `SUCCESSFUL`, ignores intents Medusa didn't create). Provider options are operator
  config; the admin only enables providers per region.
- **Saleor:** started with in-process Python plugins (a declarative `CONFIG_STRUCTURE`
  the dashboard rendered as a form, `/plugins/<id>/webhook`) and is deprecating them for
  apps: external services with a manifest, permissions, webhooks and iframe dashboard
  extensions. Payments go through synchronous webhooks (the Transactions API).
- **Shopify:** everything external is an app; *Functions* are sandboxed wasm modules
  with a typed JSON in/out contract for hot-path logic (discounts, delivery rules).
- **Vendure:** `@VendurePlugin({ entities, providers, api extensions, admin UI,
  configuration })` declares everything a plugin contributes in one place; a
  `PaymentMethodHandler` declares typed `args` the admin renders as a form per method.
- **Rust:** Bevy's `Plugin` / `App::add_plugins((A, B))` is explicit value composition;
  `inventory` and `linkme` register at link time but are unreliable or unsupported on
  `wasm32-unknown-unknown` and hide the wiring; runtime wasm plugins (wasmtime,
  components) can't run inside a Worker (no JIT for bytes loaded at run time).

Hypha takes Vendure's single descriptor and per-method settings, Medusa's core-owned
webhook route that defers to the provider, Saleor's declarative settings form, Bevy's
explicit composition, and keeps Saleor's lesson the other way round: apps for the
outside, spores only for what must be inside.

## 2. The `Spore` descriptor (`hypha-spore`)

A new crate, `hypha-spore` (layer: it may depend on core and the leaf crates whose
ports it names; hosts and spores depend on it), holds what spores are written against:

```rust
/// What a spore is: its name, its settings, what it declares. Built once at startup.
pub trait Spore: Sized + Send + Sync + 'static {
    /// "stripe": the key is `spore_stripe`, the config
    /// section `[spores.stripe]`, tables `spore_stripe_*`, scopes `spore_stripe:*`.
    const NAME: SporeName;
    /// `[spores.<name>]` plus its `HYPHA_SPORE_<NAME>_*` secrets; checked at startup.
    type Operator: OperatorSettings;
    /// The merchant's settings document (JSON Schema from `schemars`; secret fields
    /// marked `#[spore(secret)]`).
    type Merchant: MerchantSettings;

    fn build(operator: Self::Operator, env: &SporeEnv) -> Result<Self, SporeError>;

    fn scopes() -> &'static [SporeScope] { &[] }
    fn events() -> &'static [EventType] { &[] }      // documented in the webhook catalog
    fn migrations() -> Option<MigrationSet> { None }  // native `setup` runs them
    fn webhooks() -> &'static [WebhookEndpoint] { &[] }
}
```

`SporeEnv` carries what every spore may use: the host's `HttpClient`, clock, IDs,
randomness, `SecretBox` (for its merchant secrets) and the store. What a spore
*contributes* is ordinary trait implementations on types it exports, e.g.
`impl PaymentProvider for StripeProvider`, `impl SearchIndex for MeiliEngine`,
`impl EventHandler<P> for StripeWebhooks`, so its code reads like core's and is tested
the same way (testkit, fakes for HTTP).

Webhooks are their own small port:

```rust
pub trait SporeWebhook {
    /// Checks the signature over the raw body and names the delivery
    /// (the provider's event ID, for the inbox's dedupe), or refuses it.
    fn verify(&self, endpoint: &str, headers: &HeaderMap, body: &[u8], merchant: &Merchant)
        -> Result<Delivery, WebhookRefused>;
    /// Acts on a verified delivery, from the inbox, with retries: at least once.
    async fn handle<P: SporePorts>(&self, ports: &P, delivery: &StoredDelivery)
        -> Result<(), String>;
}
```

## 3. Composition in the hosts

```rust
// crates/hypha-spores/src/lib.rs, as a build generates it (committed: an empty list).
hypha_spore::spores! {
    stripe: hypha_spore_stripe::Stripe => [payments, webhooks, handlers],
    meilisearch: hypha_spore_meilisearch::Meilisearch => [search, jobs],
}
```

The macro expands to the types today's hosts write by hand: `PaymentProviders =
(ManualProvider, StripeProvider)`, the handler tuple, the engine choice (§8), the job
steps, the webhook dispatch by key, the scope and event catalogs, the migration sets,
and `GET /admin/spores`. It also checks each spore's declared `hypha_spore::API`
version against this build's at compile time, so a spore written for another version
of the spore API fails with that message rather than with type errors. What this asks
of core, once:

- **Hypha's crates the spore API names are its public surface**: `hypha-spore`,
  `hypha-core` and the leaf crates whose ports spores implement. A breaking change to
  them bumps `hypha_spore::API`, noted in the changelog spores read.
- **Payment providers and the search engine are built at startup, not `const`** (as
  handlers became for the `Mailer`), since a provider holds its HTTP client and
  operator settings.
- **Shipping's `HostRates`** takes its spore arms from the list instead of `hypha-http`
  matching on kinds.
- **The email sender** gets a `spore` arm: `[email] sender = "spore_<name>"` picks a
  spore's `EmailSender`.
- **`hypha-config`** accepts `[spores.<name>]` tables (each spore's `Operator` checks
  its own; an installed spore's missing required key is a startup error naming it; a
  section for a spore not in this build is a warning, so a config file can outlive a
  build).
- **The wasm build list and CI** cover `hypha-spore` and `hypha-spores`; Hypha's CI
  builds the empty list, and each spore's repository runs its own CI against a Hypha
  revision (a template workflow: clone Hypha at the pinned revision, generate the list
  with just that spore, `cargo test`, the wasm build, the size check).

## 4. Installation and deployment

- `hypha.json` gains `"spores"`, each `{ name, git, rev }` (`hypha spores add <git>
  [--rev]` pins the current head; `hypha spores update <name>` moves it). `hypha init`
  asks each spore's operator settings (from its `Operator` type's schema) and secrets.
- **The staged build** (the CLI, before any compile): copy the Hypha checkout (tracked
  files, no `target/`) to `<deployment>/.build/hypha`; clone each spore at its `rev`
  into `.build/spores/<name>`; write `.build/hypha/crates/hypha-spores/` (its
  `Cargo.toml` with the spores as path dependencies, its `spores!` list); append a
  `[patch."<Hypha's git URL>"]` table mapping every Hypha crate to its path, so the
  spores' git dependencies resolve to the same crates; update `Cargo.lock` for the new
  packages only (`cargo update -p` each spore, so core's pins stay). The working tree is
  never touched, and a deployment without spores builds exactly as today.
- **VPS:** the image builds from the staged copy (it's the Docker context, so the
  server never needs access to the spore repositories); **Cloudflare:** `worker-build`
  and the native `setup` binary build from it (so `setup` runs the spores' migrations).
  Secrets go into the container's environment and the Worker's `--secrets-file`.
- **Developing a spore:** its repository has Hypha as a git dependency like any
  consumer; `hypha spores dev <path>` generates a git-ignored `hypha-spores.local/`
  composition and points the workspace at it through `.cargo/config.toml`'s `[patch]`
  (git-ignored as well), so `pnpm dev` and `cargo test` include a local spore without
  changing a tracked file; `hypha spores dev --off` removes it.
- The storefront and admin need no rebuild for a spore with no client part; a spore's
  storefront package lives in its repository (`storefront/`) and is installed into the
  staged storefront build by the same step, behind `STOREFRONT_SPORES`.
- **Size budget (Workers):** each spore states its compressed wasm cost in its README;
  a spore's CI fails if its Worker (Hypha plus that spore) is over 8 MB compressed, and `hypha deploy` warns when a deployment's spores together pass it (the paid limit is 10).

## 5. Settings and secrets (SP5)

| | Operator | Merchant |
| --- | --- | --- |
| Where | `[spores.<name>]` + `HYPHA_SPORE_<NAME>_*` env | `spore_settings` row (`spore`, `document jsonb`, version) |
| Who sets it | whoever deploys | staff with `spore_<name>:write` (or the spore's declared scope) |
| Changes need | a restart / redeploy | nothing: read per request, cached briefly |
| Secrets | env only | sealed with `SecretBox` (context `spore_<name>:<field>`), write-only: the API answers `"set": true` |
| Example | Meilisearch's URL and admin key | Stripe's secret key and webhook signing secret |

`GET/PATCH /admin/spores/{key}/settings` (core routes) read and write the merchant
document through the spore's schema; `check` runs the spore's own validation (Stripe
calls Stripe once to confirm the key). Settings are a contract like configuration keys:
never renamed.

## 6. Webhooks inbox (SP6)

- `POST /hooks/{spore}/{endpoint}` (public surface, no credentials; rate limited per
  address like login, 0009): the spore isn't installed → 404; `verify` refuses → 400
  `webhook_refused` (the reason logged, not returned); verified → the delivery is
  stored in `spore_inbox` (`spore`, `delivery_id` unique, `endpoint`, `headers`,
  `body`, `received_at`, status, attempts) and the answer is 200, at once. A repeated
  delivery is a `Duplicate`, also 200.
- The inbox is worked like the outbox (0016): a dispatch step leases deliveries and
  calls the spore's `handle` with the request ports; failures retry on the dispatcher's
  schedule; the admin can see and retry them (Settings → Spores → deliveries).
- Raw bodies are kept until handled, then pruned (`jobs.prune`), since providers sign
  the exact bytes.

## 7. Reference spore: Stripe

**What core adds for it** (0027's deferred parts):
- `PaymentProvider` gains `capture`, `void`, `refund` (default: unsupported), and
  `start` receives the payment, the order's total and email, and its merchant settings.
  `NextAction::ClientSecret` stays the client-safe part; nothing server-only goes to the
  storefront (Medusa's warning).
- A workflow in `hypha-order-actions`, `record_provider_event(payment, event)`, moves a
  payment (`authorized`, `paid`, `failed`) and its order in one batch, the way
  `mark_paid` does: what the spore's webhook handler calls.
- `POST /storefront/orders/{id}/payment` (0027 §4): start again after a failed or
  abandoned attempt, with the order's access token.

**The spore** (`hypha-spore-stripe`):
- Merchant settings: secret key, publishable key, webhook signing secret (secrets
  sealed), capture mode (`automatic` | `manual`), statement descriptor. A method of
  provider `spore_stripe` per payment method set (cards, wallets via Payment Element).
- `start`: creates a PaymentIntent over `HttpClient` (`Idempotency-Key` = the payment's
  ID, `metadata.hypha_payment` = its ID) → `ClientSecret`.
- Webhook endpoint `events`: verifies `Stripe-Signature` (HMAC-SHA256 over
  `timestamp.body`, tolerance 5 min) → delivery ID = the event ID; `handle` maps
  `payment_intent.succeeded` → paid, `amount_capturable_updated` → authorized,
  `payment_failed` → failed, and ignores intents without `hypha_payment` (other
  integrations on the same account).
- `capture` / `refund` (admin buttons on the payment card, with `orders:write`).
- No tables: the payment holds Stripe's intent ID as its `reference`; the inbox
  dedupes events.
- Storefront: the repository's `storefront/` package (Stripe.js Payment Element) mounted in the
  payment step when the chosen method's provider is `spore_stripe`, confirming with the
  client secret and returning to the confirmation page, which polls the payment status.
- Tests: the provider against a fake `HttpClient` with recorded Stripe responses; the
  webhook verifier against Stripe's documented test vectors; e2e with Stripe test mode
  only when `STRIPE_TEST_KEY` is set.

## 8. Reference spore: Meilisearch

**What core adds for it**: the engine becomes a host choice. `Storage::Search` is the
spores list's engine type: an enum over the Postgres engine and each installed engine
spore (`Engine::Postgres(&PgStore)`, `Engine::Meilisearch(&MeiliEngine)`), picked by
`[search] engine = "postgres" | "spore_meilisearch"` (default postgres). The
`search_index` contract (0019) runs against every engine, so Meilisearch's results are
held to `reference::search`'s meaning; where Meilisearch ranks differently, the
contract's ranking checks say what is allowed to differ (Q6).

**The spore** (`hypha-spore-meilisearch`):
- Operator settings: URL, index prefix; admin key as `HYPHA_SPORE_MEILISEARCH_KEY`.
- `SearchIndex` over `HttpClient`: `replace` → documents API (per market index, filters
  and facets configured from the search settings), `search` → the search API, mapping
  0019's query (words, facets, price range, sort, page) and results back.
- A job step (`meilisearch_tasks`) that waits on Meilisearch's asynchronous tasks and
  reports failures; the existing search sync (0019) is unchanged: the handler marks,
  sync builds documents, the engine stores them.
- No merchant settings, no tables, no webhooks.

## 9. Spores and the rest of core

- **Events:** a spore may record its own events (`spore_stripe.dispute_opened`),
  declared in `events()` so apps can subscribe and the docs list them; they are written
  through `StageEvents` in a core batch.
- **Custom fields, policies, tax engines, shipping rates, email senders, asset
  sources:** each is a seam the descriptor can fill when that capability's port exists
  (tax and custom fields don't yet); adding one is a new seam name in `spores!`, not a
  change to existing spores.
- **Email templates:** spores don't add template keys in v1 (`TemplateKey` stays
  closed).

## 10. Not now

- **Loadable spores** (no rebuild): WASM components don't run inside a Worker; a
  Worker Loader or an interpreter (`wasmi`) could host *sandboxed* merchant logic with a
  typed JSON contract (Shopify Functions), a different tier from trusted spores.
- **Publishing Hypha's crates** (crates.io or a registry): spores would then depend on
  versions instead of git, and the `[patch]` step would go; worth it once the spore API
  is stable.
- **Custom admin pages and zones** for spores (SP10).

## 11. Open decisions

**Q1. Stripe's keys: merchant (admin, sealed) or operator (env)?** Medusa uses operator
config; Shopify and Saleor apps let the merchant connect. Recommended: merchant (SP5),
so a store connects Stripe without a redeploy and keys can be rotated in the admin; the
operator can still pin them with `HYPHA_SPORE_STRIPE_*`, which then wins and greys the
fields out.

**Q2. Where spores live.** *Decided:* each in its own repository from day one
(`invocation97/hypha-spore-meilisearch`, `invocation97/hypha-spore-stripe`), pinned by
revision in `hypha.json` and composed by the staged build (§4). Hypha's repository
being private is why the CLI clones on the operator's machine rather than letting Cargo
fetch git dependencies inside a Docker build.

**Q3. May spores add API routes?** Recommended: not in v1 (SP6): only the webhook
inbox and core routes. A spore that needs its own admin endpoints later gets a
`spore_<name>` route surface with its scopes and its own OpenAPI document, appended to
the admin's.

**Q4. Features or always-on?** Superseded by Q2: a build contains exactly the spores
its `hypha.json` lists, so nothing unused is compiled in.

**Q5. A key-value store for spores without tables?** A generic `spore_records` table
(Saleor's metadata) would cover small state without migrations. Recommended: not yet;
neither reference spore needs state beyond the inbox, settings and the payment's
reference, and the storage kit (SP9) covers real tables.

**Q6. Meilisearch and the search contract.** Recommended: the contract's matching and
filtering hold exactly; ranking is checked only for the cases 0019 calls guarantees
(exact title match first, newest on ties), since typo tolerance and proximity differ by
engine.

## 12. Build order

1. `hypha-spore`: the descriptor, `SporeEnv`, settings tiers (`[spores.*]`,
   `spore_settings`, sealed secrets, JSON Schema), open scopes, the `HttpClient` port
   (folding in Resend's and S3's), the `spores!` macro; the committed `hypha-spores`
   with the empty list reproducing today's sets; `GET /admin/spores`, the settings
   routes; the CLI's staged build, `hypha spores add/update/dev`, and a spore template
   repository with its CI workflow.
2. Startup-built payment providers and engine; the webhook inbox (route, table, dispatch
   step, admin view).
3. Meilisearch (its repository): the engine choice in core, the spore, the contract
   against a Meilisearch in compose, the CLI's install path end to end on both hosts.
4. Stripe: the port's additions, `record_provider_event`, the retry route, the spore,
   the storefront package and payment step, admin capture/refund; Stripe test mode.
5. Admin: Settings → Spores (status, schema forms, deliveries); payment method forms
   per provider.
