# 0030: Spores beside apps: in-process Rust extensions, built into the deployment

- **Status:** Proposed (2026-10-19). Amends 0029's AP1, AP8 and §12. Danilo found that
  apps alone make some extensions harder than they should be: a payment provider has to
  be hosted as its own service (a Worker with its own D1 database, or a container), with
  its own OAuth installation, encrypted token store and webhook plumbing, for what is a
  few hundred lines calling Stripe's API. Medusa's model (plugins as packages in the
  same process, listed in the project's configuration) is the better fit there. This
  note brings spores back as a second, trusted way to implement the same contracts apps
  implement. Decided: **spores own their tables and never touch core's** (Q1), **Stripe
  is a spore, and the TypeScript Stripe app stays as the example of an external payment
  app** (Q5); **a spore follows core through events** (Q2: its own `Stage*` traits and
  `also` hooks only when a spore can't do without them); the other questions as
  recommended.
- **Scope:** what a spore is and may do; one identity and one admin surface for apps and
  spores; the `Spore` trait and the `hypha-spore` facade crate; spore storage (tables,
  migrations, batches); spore routes, UI, settings and secrets; how a deployment is
  built with its spores (`hypha.json`, a generated host crate, the hosts as macros);
  testing; Stripe as the first spore.
- **Not in scope:** loading code at runtime (no dynamic libraries, no wasm plugins: 0029
  §11's Functions stay the plan for sandboxed hot-path rules); a spore registry or
  marketplace; spores adding columns to core tables or core GraphQL/REST shapes
  (custom fields, 0002 §3, are the way); TypeScript types for spore routes in
  `@hypha/sdk`.
- **Related:** 0002 §6 (the first spore design, revived here in part), 0029 (apps,
  extension points, UI targets, the first-party apps), 0027 (payment providers), 0016
  (events, handlers), 0001 (storage, `MigrationSet`, batches), 0017 (configuration),
  0025/0026 (deployments, the hosts, Workers).

## Why

0029 made apps the one way to extend Hypha, for good reasons that still hold: any
language, any host, untrusted by design, the merchant consents to what an app sees, and a
headless admin can show any app's UI. What it costs shows in the first-party Stripe app:

- **Hosting.** It's a second deployable: a Worker with a D1 database on Cloudflare, a
  container with a SQLite volume on a server, cloned and built by `hypha deploy`, bound
  to the API with a Service Binding, redeploying the API once to add the binding. Every
  operator runs it, monitors it and upgrades it in step with Hypha.
- **Plumbing that exists only because it's outside.** OAuth with PKCE and rotating
  refresh tokens, an `APP_ENCRYPTION_KEY` and its own sealed token store, signatures on
  every call both ways, per-store secrets keyed by `Hypha-Store`, an embedded settings
  page with a bridge and origin checks, a manifest. None of that is Stripe.
- **Distance from the data.** It reads the payment back through the Admin API, reports
  Stripe's events through `POST /admin/payments/{id}/events`, and keeps its own copy of
  what it needs.

A Rust crate compiled into the API would be the payment provider and the webhook route
and nothing else: no second service, no tokens, no network hop, one database. That's
what Medusa, Vendure and Saleor's former plugins offer, and what 0002 §6 first planned.

The two aren't rivals. Apps stay the right answer for anything a third party runs as a
service (an ERP connector, a reviews SaaS, a merchant's own Node backend), for anything
in another language, and for anyone who can't rebuild the API. Spores are the right
answer for infrastructure-like extensions an operator chooses at build time: payment
providers, tax engines, search engines, carriers. **Both implement the same contracts**,
so core never knows which one it's talking to.

## Decisions

| | Decision |
| --- | --- |
| SP1 | **Two runtimes, one set of contracts.** Every extension point (0029, AP3) is backed by a domain port, a Rust trait (`PaymentProvider`, a rates provider, a search engine, `EventHandler`). An app implements it over HTTP (the host's adapter calls the app: `HostPayments`, `HostRates`); a spore implements the trait in-process. Core calls the port and can't tell them apart. *Amends 0029 AP1; hard to reverse (the ports become the spore API)* |
| SP2 | **Spores are trusted code chosen by the operator**, like any Cargo dependency: not sandboxed, not consented to by staff per store. They declare the scopes they act with, the deploy shows them, and the admin lists them; staff can disable a spore but not narrow it. Apps keep 0029's trust model unchanged |
| SP3 | **A spore is an app of kind `spore`**: key `spore_<name>` (beside `app_<name>`), a row in `apps` the host creates at startup, so the admin's app list, enabling and disabling, the settings form, UI targets, `Actor::App` on its writes, metafield namespaces and the event log treat both alike. *Hard to reverse (key format)* |
| SP4 | **Spores own their tables only** (Q1, decided). A spore's migrations (module `spore_<name>`) create tables named `spore_<name>_*`; they never create, alter or write core tables, which a check enforces at migration time. A spore reads and changes core data through core's repositories and commands, exactly as routes do. Foreign keys from a spore's tables into core's are `ON DELETE CASCADE` or `SET NULL`, so a spore can never block a core delete. *Hard to reverse (the boundary)* |
| SP5 | **A spore's own writes join batches through its own `Stage*` traits**, backed by an extension slot in each batch adapter (§6); a spore joins a core command's transaction only where that command offers an `also` hook, added per command when a spore needs it; everything else goes through events, the default. (Q2, decided) |
| SP6 | **`hypha-spore` is the facade crate**: everything a spore may use, re-exported, with its own semver. Spores depend on it, not on the internal crates, so internal refactors break spores only when the facade changes. *Hard to reverse (the public Rust API)* |
| SP7 | **A deployment's spores are compiled into its API**: `hypha.json` lists them, `hypha deploy` generates a small host crate that names them (`host! { spores: [...] }`) and builds it instead of the stock host. The hosts become libraries exposing that macro. No runtime loading, on any host (Workers can't load native code; Rust has no stable ABI) |
| SP8 | **Spores build for every host**: the same hard constraints as domain crates (wasm32, no Tokio, no threads, HTTP and time through ports), checked by the deploy's build and by a spore's CI template |
| SP9 | **Stripe is a spore** (`spores/stripe`, crate `hypha-spore-stripe`, Q5 decided) and the recommended way to take cards; `hypha init` offers it. The TypeScript app (`hypha-app-stripe`) stays as the documented example of an external payment app and keeps working, but `hypha init --apps stripe` stops being the default suggestion. *Amends 0029 AP8 for Stripe* |

## 1. What others do

- **Medusa:** plugins and modules are npm packages loaded into the same Node process,
  listed in `medusa-config`, built with the project. A module owns its data models
  (its own tables) and links to other modules' through module links rather than
  foreign columns; payment, fulfilment and notification providers are modules
  implementing a provider interface. Closest to this note.
- **Vendure:** plugins are TypeScript classes in the same process that add entities,
  GraphQL extensions, strategies (payment handlers, tax, shipping calculators) and
  admin UI; core entities are extended through custom fields, not new columns.
- **Saleor:** had in-process Python plugins (payment gateways among them) and moved to
  apps (webhooks, sync webhooks for payments, an app bridge), deprecating plugins. Its
  reason is ours in 0029: a hosted, multi-tenant platform can't run merchants' code.
  Hypha is self-hosted by the operator, which is Medusa's situation, not Saleor
  Cloud's: the operator already runs our code and can choose to run more.
- **Shopify:** apps only, plus Functions (sandboxed wasm). A hosted platform, like
  Saleor's cloud.

What we take: Medusa's providers-as-modules with their own tables (SP1, SP4), Vendure's
"core entities extend through custom fields, never columns", and Saleor's lesson that
the external contract must exist anyway, so both runtimes share it (SP1).

## 2. One contract, two runtimes

| Concern | App (0029) | Spore (this note) |
| --- | --- | --- |
| Where it runs | Its own service, any language | Inside the API, Rust |
| How it gets in | OAuth install, staff approve scopes | Built into the deployment (SP7) |
| Identity | `app_<name>`, `Actor::App` | `spore_<name>`, `Actor::App` (SP3) |
| Implements a point | HTTP endpoint in its manifest, signed calls | The domain port's trait, in a tuple |
| Hears events | Webhooks (at least once, signed) | `EventHandler` in the dispatcher (at least once, in-process) |
| Its data | Its own database | Its own tables in Hypha's Postgres (SP4) |
| Calls Hypha | Admin API with its token | Core commands with a `Ctx` holding its scopes |
| Own HTTP routes | Its own server, or the app proxy | Routes in the API under `/spores/<name>/…` (§7) |
| Admin UI | Blocks, actions, settings form, embedded pages | Blocks, actions, settings form (§7) |
| Storefront UI | Elements hosted by the app | Elements served by the API (§7) |
| Secrets | Its own, encrypted under its own key | Sealed with Hypha's `SecretBox` (§8) |
| Upgrades | Its own release cycle | With the deployment |

The rows that differ are the transport; everything above the port (checkout, payment
actions, the admin's UI targets, the event log) is shared.

## 3. Identity: a spore is an app of kind `spore` (SP3)

`AppKey` (core) accepts `app_<name>` and `spore_<name>`, and says which with
`AppKey::kind() -> AppKind { App, Spore }`. `ProviderKey` (payments) accepts both again
(0029's as-built note removed the `spore_` form).

At startup, after migrations, the host makes sure each built-in spore has its `apps`
row (`register_spore`: `App::spore`, no manifest, no webhook, no OAuth tokens; its grant
is the declared scopes, approved by `Actor::System`), the way it makes the Default
location. The kind is the key's prefix, so `apps` gains no column. The row has a sealed
secret like every app's, never handed out (nothing of a spore's is signed today). A
later start with another title or other scopes updates the row (`app.grant_changed`);
whether it's enabled stays staff's.
A spore removed from the build keeps its row, marked `missing` and shown as such in
the admin, so its data and history stay readable and reinstalling it picks them up
again (Q7).

What follows from being an app:

- `GET /admin/apps` lists both, with `kind`; the admin badges spores "Built in".
- **Enable and disable** work as for apps: a disabled spore's providers report
  unavailable, its handlers are skipped, its routes answer `403 app_disabled`, its UI
  isn't discovered. A spore can't be uninstalled from the admin (it's in the build);
  the operator removes it from `hypha.json` and redeploys.
- **Scopes:** the spore's `Ctx` is `Actor::App(id)` with exactly its declared scopes, so
  its writes run the same authorization as everyone's and its events name it as the
  actor. A spore that declares `payments:write` and calls a command needing `orders:write`
  gets the same `403` an app would. That's not a sandbox (SP2: it's trusted code with
  the database connection), but it keeps the audit trail honest and catches mistakes.
- **Personal data (0029 AP10):** redaction is for data leaving the API, so a spore reads
  through repositories unredacted, as core does. Its declared `customer_data:*` scopes
  are shown to the operator and in the admin so the merchant still sees what it uses.

## 4. The `Spore` trait and the facade (SP6)

```rust
/// One spore. Built once at startup; static dispatch everywhere (tuples), like the
/// host's handlers and providers, so `Send` stays provable.
pub trait Spore: Sized + 'static {
    /// "stripe": the key `spore_stripe`, the migration module, the table prefix, the
    /// routes' prefix, the metafield namespace.
    const NAME: &'static str;
    /// What it may do, as `Actor::App` (SP2).
    const SCOPES: &'static [Scope];

    /// Its title, settings schema (§8) and UI contributions (§7).
    fn describe() -> SporeDescription;
    /// Its own tables (SP4). `MigrationSet::empty()` for none.
    fn migrations() -> MigrationSet;
    /// Builds it with the host's environment: HTTP, clock, IDs, randomness, the
    /// `SecretBox`, its own configuration section.
    fn new(env: SporeEnv) -> Result<Self, SporeError>;

    /// What it contributes to the host's tuples; `()` for none.
    type PaymentProviders: PaymentProviderSet;
    type Handlers: HandlerSet;
    fn payment_providers(&self) -> Self::PaymentProviders;
    fn handlers(&self) -> Self::Handlers;
    // later, as ports gain spore implementations: rate providers, a search engine,
    // email senders, jobs (`Step`s for the runner and the Workers cron)
}
```

The host's tuples become nested: `(ManualProvider, (StripeProvider,), ())` for a build
with Stripe and a spore with no providers. `PaymentProviders` is implemented for `()`
and for tuples of provider sets, so the nesting is invisible to checkout. Routes and UI
aren't on the trait: routes must expand with the concrete storage (§7), and UI is
answered through a `SporeUi` trait the spore implements next to it.

**`hypha-spore`** re-exports, versioned on its own (0.x, breaking changes on minor
versions until 1.0, a changelog per release):

- core's types, IDs, `Ctx`/`Scope`, `Money`, errors, `Timestamp`, ports (`Clock`,
  `IdGenerator`, `HttpClient`), `EventHandler`, `MigrationSet`;
- the domain ports spores implement (`PaymentProvider` and its types, then others);
- the commands and read repositories spores call, by domain (`hypha_spore::orders`,
  `::payments`, `::order_actions`, …): what a route handler may call, not the `Stage*`
  traits of core aggregates (a spore changes core data through commands, SP4);
- the storage kit (`hypha_spore::postgres`, feature `postgres`): `Queries`, the typed
  query helpers, `consistent_read!`, error mapping by constraint, `SporeWrite` (§6);
- the HTTP kit (feature `http`): `Caller`, `KeyedJson`, the error type, `spore_routes!`
  (§7);
- the test kit (feature `testing`): in-memory ports, `SequentialIds`, `FixedClock`, the
  migration check and a Postgres harness (§10).

A spore that reaches past the facade into an internal crate can, since they're
published, but gets no stability promise. The facade is also the list of what we must
think about before refactoring: anything it re-exports is public API.

## 5. Storage: own tables, read core through its ports (SP4, Q1)

**Migrations.** A spore's `MigrationSet` is module `spore_<name>`, passed to `migrate`
after `CORE`, append-only like every set. After applying a spore's migrations the runner
checks, from `pg_catalog`, that:

- every table, index, sequence, type and function created by that set is named
  `spore_<name>_*` (it compares the catalog before and after the set's new migrations);
- every foreign key from its tables to a table outside its prefix is `ON DELETE CASCADE`
  or `ON DELETE SET NULL`;
- it created no trigger, rule or policy on a table outside its prefix.

A failure stops startup (and `setup`) with the offending object named, before anything
is served. That makes "own tables only" mechanical for the DDL. Writing to core tables
with raw SQL can't be prevented short of separate database roles (Q6); it's outside the
contract and the facade gives no help doing it.

**Reading its own tables.** The spore defines its repository trait and accessor
(`StripeEventRepo`, `HasStripeEvents`) like a domain crate, and implements the accessor
for the host's store type. That's allowed by Rust's orphan rule: the trait is the
spore's own, so it may be implemented for a foreign type (`PgStore`). The implementation
uses the storage kit (`conn.db()`, `query_typed`, `consistent_read!`), and an in-memory
version on testkit's `Table` for its command tests.

**Reading core.** Through the repositories and commands the facade re-exports, with the
ports the request (or job, or handler) already has: `S: HasPayments + HasOrders`. A
spore that wants data next to a core object keeps it in its table keyed by the core ID
(`spore_stripe_intents.payment_id → payments(id) ON DELETE CASCADE`) or, for data other
clients should see, in a metafield under its namespace (0002 §3, when built).

**Why not let spores read core tables directly:** core's schema would become a contract
with every spore, so any migration could break deployments we can't see, and every
aggregate's rules (`restore` re-running constructors, versions, events) would be
bypassed by writes. Reading through ports costs a little convenience and keeps the
schema ours.

## 6. Writing with core in one transaction (SP5, Q2)

Three needs, from most to least common, each with its own mechanism.

### 6.1 Reacting to core: events (the default)

Most spore writes follow a core change: Stripe cancels intents when an order is
cancelled, a loyalty spore credits points when an order is paid. These are
`EventHandler`s in the spore's `Handlers`: written to the outbox in core's transaction,
delivered at least once, so the handler is idempotent (its own table keyed by the event
ID, or an idempotent call). No transaction is shared, and none is needed.

### 6.2 Several writes of the spore's own: its own `Stage*` traits

A spore command that writes several of its own rows, or its rows with events, stages
them in a batch like core does. The spore defines `StageStripeIntents` next to its repo
trait and implements it for each batch adapter, through a slot each adapter gains:

```rust
// hypha-store-postgres: the slot. A spore's write is a statement, as data, so the
// batch stays `Send` and needs no boxed futures.
pub struct SporeWrite {
    sql: &'static str,
    params: Vec<(Box<dyn ToSql + Send + Sync>, Type)>,
    /// `Any`, or `One` for a version-checked update (0 rows → `StaleVersion`).
    expect: Expect,
    /// Its unique constraints and the keys they report (`Duplicate(key)`).
    keys: &'static [(&'static str, &'static str)],
}
impl PgBatch {
    pub fn stage_spore(&mut self, write: SporeWrite);
}

// In the spore (feature "postgres"):
impl StageStripeIntents for PgBatch {
    fn stage_intent(&mut self, intent: &mut Intent) {
        self.stage_spore(intents::insert_or_update(intent));
    }
}
```

`PgBatch::commit` applies spore writes after every core write, in the order staged
(rank last: spore rows may reference core rows, never the reverse, SP4). `InMemoryBatch`
gains the same slot as a closure over the spore's in-memory tables, so the spore's
command tests run without Postgres. The spore's commands are then generic over
`S: HasBatch<Batch: StageStripeIntents + StageEvents>`, the pattern every core
workflow uses; in the generated host, `PgStore`'s batch satisfies it because the impl
exists.

A spore stages its own events through core's `StageEvents`, with types under its
namespace (`spore_stripe.intent_created`): dispatched to handlers and to apps
subscribed to them like any event (Q8).

### 6.3 Joining a core command's transaction: an `also` hook, per command

Rarely, a spore's row must commit with a core change or not at all. A core workflow
that a spore needs to join offers a variant taking extra staged writes:

```rust
pub trait AlsoStage<B> {
    fn stage(self, batch: &mut B);
}
impl<B> AlsoStage<B> for () {
    fn stage(self, _: &mut B) {}
}

// hypha-order-actions, when a spore needs it:
pub async fn record_payment_event_also<S, A>(…, also: A) -> Result<…, CommandError>
where
    S: OrderActionPorts,
    A: AlsoStage<<S as HasBatch>::Batch>;
```

The command stages its own writes, then `also.stage(&mut batch)`, then commits; on a
lost race it retries, so `also` must be cheap to rebuild (taken as a closure producing
the writes, in practice). We add the hook to a command only when a real spore needs it,
and keep the plain function as the hook with `()`. Not adding it everywhere up front:
each hook is public API (SP6), and most needs are 6.1's.

**Stripe needs none of 6.2 or 6.3:** reporting Stripe's events is
`record_payment_event` with Stripe's event ID as the idempotency key, which core
already dedupes in the same transaction (`IdempotencyRecord`), and cancelling intents is
a handler (6.1). The first spore to need 6.2 builds the slot; 6.3 waits for its first
real case.

## 7. Routes and UI

**Routes.** A spore lists its routes with the same table shape as core's:

```rust
hypha_spore::spore_routes! { stripe_routes:
    [public, POST, "/spores/stripe/webhook", webhook::receive],
    [admin, GET, "/admin/spores/stripe/payments/{id}", admin::payment],
    [storefront, GET, "/storefront/spores/stripe/elements/payment.js", elements::payment],
}
```

It expands to a macro the generated host invokes with its concrete storage, for the
reason `router!` exists: axum can only prove a handler's future `Send` for a concrete
storage type. Handlers take `Caller<Admin>` / `Caller<Storefront>` and call
`caller.authenticate(&state)` like core's; `public` routes take no credentials and are
rate limited per client address by one shared rule, `Rule::SporePublic` (a new `Rule`
for each spore would change the Workers' Rate Limiting bindings per build). Paths are under `/spores/<name>/`,
`/admin/spores/<name>/` and `/storefront/spores/<name>/`, checked at build time, so a
spore can't shadow a core route. A disabled spore's routes answer `403 app_disabled`.
The generated host's `openapi` command includes them (a tag per spore); the committed
documents in this repository describe the stock build only.

**Admin UI.** The same targets and forms as apps (0029 §5–§6), answered in-process:

- **Blocks:** the spore implements `SporeUi::block(target, context, ports, ctx)`
  returning a block tree (`packages/app-blocks`' format v1, checked by `blocks.rs`).
  `GET /admin/app-extensions` and the blocks route ask two sources, installed apps over
  HTTP and spores in-process, and merge them; frontends can't tell.
- **Actions:** declared in its description; pressing one calls `SporeUi::action`. Payment
  actions need nothing: capture, void and refund already go through `PaymentProvider::act`.
- **Settings form:** its JSON Schema, saved through core (§8).
- **Embedded pages:** not for spores in v1. A page needs an origin of its own and a
  bridge; a spore that needs one is better as an app, or serves its page from its admin
  routes later (Q9).

**Storefront elements.** A spore serves its element's module from its storefront routes
(the built JavaScript embedded with `include_str!`, cached by version), and its
`NextAction::ClientWidget` names it. The element is on the API's origin, which the
storefront already trusts.

## 8. Settings and secrets

Spores don't each build a settings store. Core gains one: `built_in_settings` (one row per
spore: its JSON, version-checked), written by `PUT /admin/apps/{key}/settings`
(`apps:write`) for a spore key, which:

1. checks the body against the spore's schema, then the spore's own
   `check_settings` (a Stripe key that starts with `sk_`, a webhook secret with
   `whsec_`);
2. seals the fields the schema marks `"x-hypha-secret": true` with the host's
   `SecretBox` (`HYPHA_SECRET_KEY`, as 2FA secrets are);
3. answers the settings with secret fields replaced by `{"set": true}`: write-only, as
   the Stripe app's are.

The spore reads them unsealed through `HasSporeSettings`, typed by its own
`Deserialize` struct. Method-level settings that aren't secret (capture automatically
or manually) stay on the payment method (0027), checked by the provider's
`check_settings` as today.

Operator settings (things that belong to the deployment, not the merchant: an API base
for tests, a timeout) go in `hypha.config.toml` under `[spores.<name>]`, handed to the
spore as a `toml::Table` it parses in `new`; `config` prints them and `check` reports
the spore's errors with their keys. Spore keys are its own contract with operators,
like core's (0017).

## 9. Building a deployment with spores (SP7)

**Listing them.** `hypha.json`:

```json
"spores": [
  { "name": "stripe" },
  { "name": "reviews", "git": "https://github.com/acme/hypha-spore-reviews", "rev": "4f2c1a9" },
  { "name": "loyalty", "crate": "acme-hypha-loyalty", "version": "0.4.1" }
]
```

A first-party name alone (`stripe`) is this checkout's `spores/<name>`. `hypha init
--spores stripe`, `hypha spores add|remove|update`, like `apps`.

**The generated host.** `hypha deploy` writes `<deployment>/.build/host/`:

```toml
# Cargo.toml
[package]
name = "hypha-deployment"
[dependencies]
hypha-host-axum = { path = "../hypha/crates/hypha-host-axum" }
hypha-spore-stripe = { path = "../hypha/spores/stripe" }
hypha-spore-reviews = { git = "https://github.com/acme/hypha-spore-reviews", rev = "4f2c1a9" }
```

```rust
// src/main.rs
hypha_host_axum::host! {
    spores: [hypha_spore_stripe::Stripe, hypha_spore_reviews::Reviews],
}
```

and the same for Workers (`hypha_host_workers::worker! { spores: [...] }`, built with
`worker-build`). Its `Cargo.lock` starts as a copy of the workspace's, so core's
dependencies resolve exactly as they're tested, and stays in the deployment folder
between deploys. `docker/api.Dockerfile` takes the crate to build as an argument; the
build is where it is today (the server over SSH, or here).

**The hosts become libraries plus a macro.** `hypha-host-axum`'s `main.rs` moves into
the library behind `host!`, which expands, for the listed spores, the `Handlers` and
`PaymentProviders` types, the router (`router!` merged with each spore's routes), the
migration list (`[CORE, Stripe::migrations(), …]`) and the startup that creates each
spore's `apps` row. Our own binary is `host! { spores: [] }`, so the stock build doesn't
change. Macros, not generic functions, for the same `Send` reason as `router!`.

**Developing in this repository.** The workspace's hosts take first-party spores behind
Cargo features (`--features spore-stripe`), so `pnpm dev`, the route tests and CI build
and test them with core. CI builds every first-party spore for wasm (the command list in
`CLAUDE.md` grows by each).

**Costs, said plainly.** A deployment with spores compiles its own API (minutes, about
3 GB of memory, as today: deploy already compiles it); a spore incompatible with the
Hypha version fails the build, not the running store, which is the better place to fail.

## 10. Testing a spore

- **Command tests** on in-memory ports: `hypha_spore::testing` re-exports
  `InMemoryPorts`, `SequentialIds`, `FixedClock`, and the in-memory batch slot (§6.2).
- **Storage:** the spore runs its own repository contract against Postgres with
  `hypha_spore::testing::postgres` (a fresh schema per test with `CORE` and its own
  migrations, and the migration check of §5).
- **Routes:** `TestApp` (the route tests' harness) gains `with_spores!`, so a spore tests
  its routes on a real host with the recording mailer and outbox.
- **A template** (`spores/template`, later `cargo generate`) with all three, a wasm
  build in its CI, and the `CLAUDE.md` sections a coding agent needs to extend it.

## 11. Stripe as a spore (SP9, Q5)

`spores/stripe` (`hypha-spore-stripe`), ported from `hypha-app-stripe`, keeping its
behaviour (its README is the specification) and dropping its plumbing:

| The app | The spore |
| --- | --- |
| `payments.start` / `.capture` / `.void` / `.refund` endpoints | `StripeProvider: PaymentProvider` (`start`, `act`) |
| OAuth, token store, `APP_ENCRYPTION_KEY`, D1/SQLite | None: `Actor::App` with its scopes, settings sealed by core (§8) |
| Stripe webhooks at `/stripe/webhook/<store>` | `POST /spores/stripe/webhook`, verified with the sealed signing secret, then `record_payment_event` with Stripe's event ID as the idempotency key |
| Reporting through `POST /admin/payments/{id}/events` | The same command, called in-process |
| `order.cancelled` webhook → cancel intents | An `EventHandler` on `order.cancelled` |
| Settings page in an iframe, bridge, origin checks | The settings form from its schema (secret key, publishable key, webhook secret) |
| `order.payment` block through the proxy | `SporeUi::block` |
| `<stripe-payment>` element hosted by the app | The same module, served from `/storefront/spores/stripe/elements/payment.js` |
| `fetch` to Stripe | The host's `HttpClient` (works on both hosts) |

What carries over as code: the amount conversion (`stripe/amounts.ts` → Rust, with its
tests), the signature check, the event → payment-event mapping (refunds counted by
`re_…` ID), the idempotency-key scheme, the element's JavaScript as built.

The TypeScript app stays in its repository, documented in `docs/apps/` as the example
of an external payment app (what a provider in another language does). `hypha init`
suggests the spore; `--apps stripe` still works.

## 12. What changes in 0029

- **AP1** becomes: "Apps are the public extension model for services; spores (0030) are
  the trusted, compiled-in one. Both implement the same contracts."
- **AP8:** Stripe is a spore (SP9); Meilisearch stays an app until search's engine port
  is ready for a spore (Q10).
- **§12** ("What replaces spores") is superseded by this note. The `spore_` reservations
  it left unused are used again; `ProviderKey` takes `spore_` keys again.
- Everything else stands: extension points, the manifest, OAuth, UI targets and blocks,
  personal-data scopes, co-located apps.

## 13. Open decisions

**Q1. May spores use core's tables?** *Decided:* no; their own tables only (SP4).

**Q2. Writing with core in one transaction.** *Decided:* events by default (§6.1); the spore's own `Stage*` traits over a statement slot in `PgBatch` and a
closure slot in `InMemoryBatch`; an `also` hook on a core command only when a spore
needs to join it. Alternatives considered: (a) letting spores stage core aggregates
directly (bypasses workflow rules such as the order's derived `financial_status`); (b)
boxed async writes in the batch (`!Send` on Workers, and no typed test double).

**Q3. Routes.** *As recommended:* `spore_routes!` expanded by the generated host, under
`/spores/<name>/` prefixes. *As built:* the host lists each spore's route macro
(`routes: [...]`) beside `spores`, and public routes share `Rule::SporePublic`.

**Q4. The facade.** *As recommended:* `hypha-spore`, 0.x with breaking changes on minor
versions, a changelog per release.

**Q5. Stripe.** *Decided:* a spore, recommended; the TypeScript app as the example.

**Q6. Database roles.** Should a spore's queries run as a Postgres role that can only
write its prefix? Real enforcement of SP4, but a second role and connection pool per
spore, and Hyperdrive has one connection string. Recommended: not now; the DDL check
and review are enough while spores are first-party or chosen by the operator.

**Q7. Removing a spore.** Recommended: removing it from `hypha.json` leaves its tables
and `apps` row (`missing`); `hypha spores drop <name>` (asks, then drops its tables and
its migration rows) for when the data should go.

**Q8. Spore events.** Recommended: types namespaced `spore_<name>.<event>`, payloads
versioned like core's, deliverable to apps that subscribe (a reviews spore's
`review.created` reaching an email app). Apps' subscriptions already take event names;
the grant's `event_scope` needs a rule for them (the spore's declared read scope).

**Q9. Embedded pages for spores.** Recommended: not in v1 (blocks, actions and the
settings form cover Stripe); revisit with the first spore that needs a page.

**Q10. Which ports get spore implementations next.** Recommended order: payments (with
Stripe), shipping rates (carriers), the search engine (Meilisearch as a spore beside the
app), email senders (already compiled in: they become spores' to add), tax engines.

## 14. Build order

1. `AppKey`/`ProviderKey` take `spore_` keys; `kind` on apps (from the key);
   `register_spore`, the command the host's startup calls; staff limited to enabling
   and disabling a spore (`app_built_in`). *Done.* Left for step 2, where a host first
   has spores: calling it at startup, the `missing` mark (Q7), and enable/disable
   honoured by providers, handlers and UI discovery.
2. Hosts as libraries: `host!` (and `worker!`) with an empty spore list producing
   today's binaries, nested provider and handler tuples, the migration list. *Done*
   (see "As built: step 2" below), with the startup registration step 1 left. Left for
   later: the `missing` mark (Q7); enable/disable honoured by providers, handlers and UI
   discovery; the host's environment for `Spore::new`; `TestApp::with_spores!` (§10);
   Cargo features for first-party spores in this workspace (§9).
3. `hypha-spore` facade, the migration check (§5), `spore_routes!`, `SporeUi` and the
   two-source UI discovery, core's `built_in_settings` with sealed fields and the admin's
   settings form for spores. *Done* (see "As built: step 3"), with step 2's leftovers:
   `SporeEnv`, enable/disable honoured by providers, handlers, routes and UI, and the
   route tests' `app_with_spores::<D>()`. Still left: the `missing` mark (Q7), Cargo
   features for first-party spores (§9), storefront elements from spores, the batch
   slot and `consistent_read!` for spores (§6.2, with their first user).
4. `spores/stripe` behind `--features spore-stripe`, with command tests, Postgres and
   route tests; `scripts/e2e.sh` steps for its routes (with a fake Stripe, as the app's
   tests use).
5. The CLI: `hypha.json` `spores`, the generated host crate for both targets,
   `hypha spores …`, `init --spores`.
6. Docs: `docs/spores/` (writing a spore, the facade, the template), 0029 amended, the
   Stripe app's README pointing at the spore.

The batch slot (§6.2) and `also` hooks (§6.3) are built by the first spore that needs
them, not in this list.

## As built: step 3 (facade, migration check, settings, routes, UI)

- **`SporeEnv` (§4).** `Spore::new(env: SporeEnv) -> Result<Self, SporeError>`. The
  env is the spore's clock, ID generator, randomness, `SecretBox` (it implements those
  ports, so `&env` goes wherever a command asks) and HTTP client
  (`hypha_spore_core::HttpClient`: method, URL, headers, body, timeout, answer cap). The
  HTTP is the host's outbound client for apps (`hypha_http::spores::spore_host` over
  `HostCalls`): no redirects, natively only public addresses unless
  `outbound.allow_private`. Not the media storage's `HttpClient`, which has no request
  body. Its `[spores.<name>]` table comes from `hypha.config.toml` as written
  (`config()`, `config_as::<T>()`); `hypha-config` keeps every `spores.*` table as is
  (`Config::spores`) and the host refuses one for a spore it wasn't built with.
  `SporeSet::build(host, config)` builds them in order; the first `SporeError` stops
  startup as `spore "stripe": <error>` (natively and on Workers). The built spores are
  kept (`Storage::spores()`, the same tuple type), and a route handler reaches its own
  with `hypha_http::spores::spore::<Stripe, S>(&state)` (an `Any` downcast by name).
- **The migration check (§5)** is in `migrate` (`spore_check.rs`): for each new
  migration of a `spore_*` set, the current schema's catalog (relations with the table
  an index is on, types, functions, triggers, rules, policies, constraints, columns) is
  read before and after it, in its transaction. Problems roll it back and stop startup
  and `setup`, naming each object: anything not named `spore_<name>_*` (the longest
  module prefix wins, so `spore_a_b_*` isn't `spore_a`'s), an index, trigger, rule,
  policy, constraint or column added to a table outside the prefix, a foreign key from
  its tables to another table that isn't `CASCADE`/`SET NULL`, anything outside the
  prefix dropped. Only the current schema is read (other schemas may be other
  databases', as the route tests' are). Tested on Postgres with a well-behaved spore
  and four rogue ones.
- **Settings (§8).** `built_in_settings` (migration 0056, named outside `spore_*`, which is spores' own: key, version, JSON), repository
  `SporeSettingsRepo` / `HasSporeSettings` in `hypha-apps` (contract scenario
  `spore_settings_repo`, in memory and on Postgres; no outbox events). The schema is a
  subset of JSON Schema, `hypha_apps::SettingsSchema`: an object of scalar fields
  (`string`, `integer`, `number`, `boolean`; `enum`, `minLength`/`maxLength`,
  `minimum`/`maximum`, `title`, `description`, `default`, `examples`, `format`,
  `required`, `additionalProperties: false`), checked when the host starts so every
  admin can draw it; not a general validator (no `pattern`, nesting or `oneOf`). `PUT
  /admin/apps/{key}/settings` (`{"settings": {…}}`, `apps:write`) checks the schema,
  then `Spore::check_settings` with secrets in plain text, then seals the
  `"x-hypha-secret": true` fields with the host's `SecretBox`, context
  `spore_<name>/<field>`, stored as `{"sealed": "<hex>"}`. A secret left out keeps its
  value, `null` clears it. `GET` (`apps:read`) and the `PUT` answer `{app, schema,
  settings, updated_at}`, secrets as `{"set": bool}`. A key that's no spore of this
  build (or an app) is `404 app_not_found`, a spore without a schema
  `app_settings_not_found`, bad values `422 settings_invalid` with
  `details.errors: [{path: field, message}]`. The spore reads them through
  `env.settings::<T>(&ports)`.
- **The facade, `hypha-spore` 0.1.0 (SP6).** Root: `Spore`, `SporeEnv`, `SporeError`,
  `SporeUi`. Modules: `env`, `client` (HTTP), `settings`, `ui`, `core` (IDs, money,
  `Ctx`/`Scope`, time, ports, `EventHandler`), `migrations`, `payments` (the provider
  port, reading payments and methods), `orders` (reading), `order_actions`
  (`record_payment_event`); features `postgres` (`PgStore`, `Queries`, `Db`,
  `PgConnection`, `spore_repo_error`; `PgStore::conn()` became public for spores'
  repositories), `http` (`Caller`, `JsonBody`, `PathId`, `Query`, `ApiError`, the state
  types, `spore_routes!`, `spore`, `utoipa`) and `testing` (in-memory ports, clocks,
  IDs, a `SporeHost`). No `Stage*` trait of a core aggregate. It builds for wasm with
  every feature (in CI's list).
- **Routes (§7).** `hypha_http::spore_routes! { crate::stripe_routes for Stripe: [public,
  POST, "/spores/stripe/webhook", webhook::receive], … }`, invoked at the spore's crate
  root (handler paths are from there, `module::handler`, as `route_table!`'s). A `const`
  assertion checks each path is under its surface's prefix and the spore's name when
  the spore builds; a test keeps core's routes out of the three prefixes, so a spore
  can't shadow a core route. It defines the exported macro `stripe_routes!` (`router
  <storage>`, `rows`), which `host! { spores: [Stripe], routes:
  [hypha_spore_stripe::stripe_routes] }` and `worker!` expand with the concrete storage
  and merge into the router; startup refuses routes of a spore not in the list. The
  `routes` list is apart from `spores` because a `macro_rules!` can't find a macro from
  a type. Each route is wrapped (`hypha_http::spores::gate`): public ones are rate
  limited per client address by `Rule::SporePublic` (`[rate_limits] spore_public`,
  120 then one every 250 ms; binding `RL_SPORE_PUBLIC`, slot 11, in `wrangler.toml`),
  then every one answers `403 app_disabled` while the spore is off (one read of its app
  per request). Handlers carry `#[utoipa::path]`; the host's `openapi` command
  documents them under the tag `spore_<name>`, operation IDs
  `<surface>.spore_<name>.<handler>`, public ones in the admin document without
  credentials. The committed documents (the stock build) only changed for the settings
  routes and the blocks and actions routes' `app` pattern, which takes `spore_` keys.
- **UI (§7).** `SporeUi<P>` (next to `Spore`, `P` the host's ports): `blocks()` and
  `actions()` declare `(id, target)` (and an action's label and input schema), `block`
  and `action` answer with a `UiCall` (the target, the context, the staff member, and
  a `Ctx` that is the spore's app acting with its scopes as far as the caller holds
  them, as an app's session token). `GET /admin/app-extensions` merges enabled spores'
  blocks, actions and settings forms (`url` is the blocks route, `proxy` false;
  `secret_fields` from the schema) where their declared scopes read the target, as
  apps' grants must. The blocks and actions routes answer a `spore_` key in-process:
  `app_disabled` (403) when off, the caller and the spore must read the target, the
  tree is checked with `check_block` (an invalid one, or `UiError::Failed`, is `502
  app_unavailable`), nothing is written to the extension call log (no call left the
  process). No embedded pages, session tokens or proxy for spores (Q9); no storefront
  elements yet.
- **Enable and disable** (step 2's leftover). Providers: `HostPayments` reports a
  disabled spore's provider unavailable (so new methods with it are refused), `start`
  is `payment_provider_unknown`, actions `payment_action_unsupported`, as a disabled
  app's. Handlers: each spore's set is wrapped in `Gated`; fan-out still makes their
  deliveries, and running one while the spore is off fails with "spore_<name> is
  disabled", retried on the dispatcher's schedule like a disabled app's webhook
  (72 hours by default), so nothing that happened while it was off is lost if it's
  turned back on within that window; after it they're failed like any delivery.
  Routes and UI: above.
- **Tests.** The test spore (`hypha-host-axum`'s `src/test_spore.rs`: a table, a
  handler, a payment provider, a config section, settings with a secret, a block, a
  broken block, an action, a route per surface) runs in `route_tests/spores.rs` on a
  host built with it (`app_with_spores::<D>()`, the `TestApp::with_spores!` of §10 as a
  generic function: `TestApp<D>` and the send helpers are generic over the deployment).

## As built: step 2 (hosts as libraries)

- **`hypha-spore-core`** holds `Spore` (`NAME`, `SCOPES`, `title()`, `migrations() ->
  MigrationSet`, `new()`, `type PaymentProviders` / `type Handlers` with
  `payment_providers()` / `handlers()`; `()` for none) and `SporeSet`, implemented for
  `()` and tuples of up to 8 spores (registrations, migrations, `build()`), plus
  `check()` (bad names, repeats, migrations under another module than `spore_<name>`).
  Its own crate because core can name neither `MigrationSet` nor `PaymentProviders`; the
  facade (step 3) re-exports it. `new()` takes no environment yet: §4's `SporeEnv` and
  `SporeError` come with settings in step 3, and change the signature.
- **Nested sets are a `hypha_core::Union`**, not bare tuples of tuples:
  `Union((core, Union((a, b))))`. `EventHandlers` and `PaymentProviders` read a union as
  one set (the first set with the name or key answers). Bare nesting can't work for
  handlers: a tuple of handlers is already a set, and making a tuple of sets one too
  overlaps, since another crate may implement `EventHandler<ItsPorts>` for a tuple.
  `PaymentProviders` gained `keys()` and an impl for `()`.
- **Native host:** `main.rs` is `hypha_host_axum::host! { spores: [] }`. `host!` uses
  `deployment! { enum Deployment { spores: [...] } }`, which declares an uninhabited type
  implementing `hypha_host_axum::Deployment` (`type Spores`, `router()`, `dispatch()`):
  the two places that need `Send` proven for the concrete providers and handlers. The rest
  (`start.rs`: subcommands, startup, serving, the other jobs) is generic over it.
  `PgStorage<D>` joins core's handlers and providers with the spores'. A start migrates
  `[CORE, spores' sets…]`, refuses bad spores before migrating and repeated handler
  names or provider keys after building them, then registers each spore's app
  (`register_spore` as `Ctx::system()`). A route test builds a host with a test-only spore
  (`route_tests/spores.rs`).
- **Workers host:** the same shape. `hypha_host_workers::worker! { spores: [...] }`
  expands the deployment, its router and the `#[event(fetch)]` / `#[event(scheduled)]`
  exports, which call the library's generic `fetch::<D>` / `scheduled::<D>`. The isolate's
  setup is kept as `Rc<dyn Any>` (a `thread_local` can't be generic) and downcast to
  `Isolate<D>`. The stock Worker is `worker! { spores: [] }` behind the crate's default
  `stock` feature. A deployment's crate turns it off (`default-features = false`), so the
  events are exported once, and depends on `worker` itself, since `#[event]` expands to
  `::worker` paths. A Worker runs no migrations, so its database's `setup` must come from a
  native host built with the same spore list (step 5's generated crate builds both).
