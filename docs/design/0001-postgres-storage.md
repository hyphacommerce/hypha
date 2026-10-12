# 0001: Postgres storage, and how commands stay consistent

- **Status:** Accepted (2026-10-12). Decisions D1–D6 are recorded in the summary below and in place.
- **Scope:** the first real storage adapter (`hypha-store-postgres`, via sqlx), and the
  changes to ports and commands it forces. D1 is not built here, but nothing below may
  rule it out.

## Decisions

| | Decision |
| --- | --- |
| D1 | The version is a field on the aggregate root |
| D2 | Unique keys are `&'static str` constants owned by each domain crate, each covered by a contract test |
| D3 | Catalog products and price-list fixed prices move behind narrow, batched port methods first (step 1) |
| D4 | **Changed 2026-10-13:** `tokio-postgres`, not sqlx, using only `query_typed` / `batch_execute`, so the same adapter also runs on Cloudflare Workers through Hyperdrive. See section 7 |
| D5 | Docker: `compose.yaml` at the repo root (Postgres 18) |
| D6 | `Money` amounts are capped at ±(2⁵³ − 1) |

## 1. Problem

Today every repo is in memory, and commands enforce consistency by *reading, checking,
then writing*. In memory that's safe, because nothing runs concurrently. With a real
database it isn't:

| Rule | Where it's checked now | Race with a real DB |
| --- | --- | --- |
| Product handle is unique | `create_simple_product`: `get_by_handle`, then `save` | Two requests both see "free", both insert |
| A country belongs to one market | `create_market`: `list_all`, then `save` | Two markets claim `DE` |
| One catalog per market | `create_catalog`: `get_by_context`, then `save` | Two catalogs for one market |
| Variant IDs are unique across products | Only within one `Product` | Not checked at all across products |
| No lost updates | Nowhere | Two `add_line` calls on one cart: both load, both save, one line disappears |

Later, checkout will need to write **several aggregates at once** (create an order and
complete the cart, and later reserve stock).

## 2. Constraint that shapes everything: D1

Cloudflare D1 has **no interactive transactions** (`BEGIN … read … COMMIT` across awaits
fails). It offers `batch()`: a fixed list of statements run atomically, with no reads in
between ([details](https://event-driven.io/en/cloudflare_d1_transactions_and_tradeoffs/)).
Workers futures are also `!Send`, which our ports already allow.

So the design must not depend on holding a transaction open while the command thinks.
Everything below works with **"read without locks, then one atomic write that fails if
the world changed"**. Postgres can do more, but we deliberately don't use it at the
port level.

### 2.1 Other D1 facts that shape the design

Source: [D1 docs (llms-full.txt)](https://developers.cloudflare.com/d1/llms-full.txt),
read 2026-10.

- **Read replicas (Sessions API, beta since 2025-04-10):**
  - Replicas are eventually consistent, and writes always go to the primary.
  - Version checks (3.2) make stale reads safe: they only cause a `StaleVersion`
    conflict, never a lost update.
  - **Rule:** commands that write read from the primary (`first-primary`). Read-only
    queries may use replicas.
- **Limits:**
  - **100 bound parameters per query.** Bulk writes must be split into chunks
    (relevant to D3).
  - **2 MB per row or string.** Rules out one JSON document per large aggregate.
  - **1000 queries per Worker invocation (50 on Free).** Ports will need batch reads
    such as `get_many(ids)`. For example, `load_products` runs one query per cart line
    today.
  - **10 GB per database, not raisable, but 50,000 databases per account.** D1 is built
    for one database per store or tenant. Multi-tenancy is out of scope here, but it
    must stay possible.
  - **30 s** per query or batch call. Each database is single-threaded.
- **Numbers pass through JavaScript: "52-bit precision"** (exact integers only up to
  2⁵³ − 1). `Money.amount` is `i64` (see D6).
- Foreign keys and `CHECK` are enforced by default. Unique indexes are standard SQLite.
  The constraint approach in 3.1 works the same on Postgres and D1.
- Migration files must not contain `BEGIN`/`COMMIT`; D1 wraps them itself.
- Time Travel restores to any minute in the last 30 days, giving free point-in-time
  backups.

## 3. Proposal

### 3.1 The database enforces uniqueness; commands keep their pre-checks

- Every uniqueness rule becomes a constraint:
  - `products.handle UNIQUE`
  - `variants.id PRIMARY KEY` (across all products)
  - `market_countries.country PRIMARY KEY`
  - `catalogs (context_kind, context_id) UNIQUE`
- Commands **keep** their read-then-check. It gives the precise error in the normal case
  (for example, *which* market owns `DE`). The constraint catches the rare race.
- Adapters translate a constraint violation into a typed repo error (3.3). Commands map
  that to the same domain error as the pre-check. So a race and a normal duplicate look
  identical to clients (`handle_taken`, `country_in_other_market`, `catalog_exists`).

### 3.2 Optimistic concurrency with a version per aggregate

- Each aggregate root gets a `version: u64`, starting at 0.
- `save` becomes two operations:
  - `insert(&T)` fails if the ID or a unique key already exists.
  - `update(&T)` runs `UPDATE … SET version = version + 1 WHERE id = $1 AND version = $2`.
    If zero rows match, it fails with `StaleVersion`.
- A lost update becomes an explicit `409 concurrent_modification`.
- **Decided: D1** where the version lives.
  - **(a) A field on the aggregate.** Read with `product.version()`. The repo bumps it
    after a successful save. Simplest to use, and it's how most DDD codebases do it.
  - **(b) A `Versioned<T> { value, version }` wrapper** returned by `get`. The domain
    stays free of persistence concerns, but every command passes the version around.
  - *Recommendation: (a).* Hypha's aggregates already need a "rebuild from storage" path
    (3.4), and the version fits naturally there.

### 3.3 Repo errors become typed

```rust
// hypha-core
pub enum RepoError {
    Backend(String),                 // 500, message hidden (as today)
    StaleVersion,                    // 409 concurrent_modification
    Duplicate(UniqueKey),            // mapped by the command to a domain error
}
```

- **Decided: D2** how `UniqueKey` is typed.
  - **(a) `&'static str` constants owned by each domain crate**, for example
    `hypha_products::repo::keys::HANDLE = "products_handle_key"`. Adapters name their DB
    constraints the same way. Lightweight, and core stays domain-free.
  - **(b) An associated type per repo trait**, for example
    `ProductRepo::insert -> Result<(), RepoError<ProductKey>>` with
    `enum ProductKey { Handle, VariantId }`. Fully typed and exhaustive, but every repo
    signature gets more complex.
  - *Recommendation: (a)*, plus a contract test (3.6) for each key, so a typo is caught by
    tests rather than in production.
- `ErrorCode`: `StaleVersion` becomes `concurrent_modification`. A `Duplicate` that no
  command handled falls back to `conflict`.

### 3.4 Rebuilding aggregates from storage: snapshots

Aggregates have private fields and validated constructors, so an adapter can't fill them
in directly. Each aggregate gets a plain-data **snapshot** in its own crate:

```rust
pub struct ProductSnapshot { pub id, pub version, pub handle: String, pub status, pub options, pub variants, … }
impl Product {
    pub fn snapshot(&self) -> ProductSnapshot;
    pub fn restore(s: ProductSnapshot) -> Result<Product, ProductError>; // re-validates
}
```

- Adapters only ever see snapshots. This works the same for Postgres tables, D1 and JSON.
- `restore` re-runs the invariants. Bad data in the database fails loudly and doesn't
  produce an invalid `Product`. That's the same rule as "never `Deserialize` a validated
  type directly".

### 3.5 Schema shape

- **Normalized tables, not one JSONB document per aggregate.** We need indexed lookups
  (`get_by_handle`, `get_by_variant`, `get_by_context`), unique constraints on child
  rows (variant IDs, countries) and, eventually, D1, which has no JSONB.
- **Types:**
  - IDs: native `uuid`.
  - Money: `amount BIGINT` plus `currency TEXT` with a `CHECK` on the known codes.
  - Basis points: `INTEGER`.
- **Decided: D6, money range.** D1 can't represent integers above 2⁵³ − 1 exactly, and
  neither can any JavaScript client reading our JSON, such as the admin panel.
  - **(a) Cap `Money` amounts in the domain at ±(2⁵³ − 1)**, about 90 trillion EUR in
    cents. The `checked_*` operations return `Overflow` beyond that. One rule, safe
    everywhere.
  - **(b) Keep the full `i64`.** Store amounts as TEXT in D1 and send them as strings in
    JSON. Awkward for every client.
  - *Recommendation: (a)*, done in step 2. It's a small core change.
- `created_at` / `updated_at` are filled by the database. Adapter-only, not in the domain.
- **Saving an aggregate:** the root row plus its small children (variants, options,
  countries, cart lines), with the children replaced, in one transaction. On Postgres
  that's a short transaction inside the adapter. On D1 it's one `batch()`. Neither needs
  a transaction across awaits, because the version check makes the whole write
  all-or-nothing.
- **Decided: D3, large collections.** `Catalog.products` and `PriceList.fixed_prices`
  can hold 100k+ entries. Today every `resolve_price` call loads the **whole** catalog
  and price list into memory. That's fine in memory, but wrong for "scalable to large
  catalogs".
  - **(a) Now:** give them their own tables and narrow port methods. For example
    `CatalogRepo::includes(catalog, product)`, `add_products`, `remove_products`, and
    `PriceListRepo::fixed_price(list, variant)`. `Catalog` and `PriceList` would no
    longer hold the collections.
  - **(b) Later:** keep the current aggregates, store children in tables anyway, and
    revisit when it hurts.
  - *Recommendation: (a), as its own first step.* It changes the shape of
    `resolve_price`'s inputs, and doing it before a schema exists is far cheaper than
    after.

### 3.6 One contract test suite for every adapter

- `hypha-testkit` gets generic contract tests, for example
  `pub async fn product_repo_contract<R: ProductRepo>(make: impl Fn() -> R)`. They cover:
  - insert and get round-trips
  - duplicate handle → `Duplicate(HANDLE)`
  - a save from an old version → `StaleVersion`
  - `get_by_variant`, and so on
- The **in-memory repos must pass them too**. They gain version checks and unique
  indexes, so command tests keep matching production behavior.
- The Postgres crate runs the same suite against a real database when `DATABASE_URL` is
  set, and skips it otherwise. `scripts/e2e.sh` then runs unchanged against a host that
  uses Postgres.

### 3.7 Writing several aggregates at once (not built now)

- **Rule for now:** a command writes at most one aggregate. Every command today already
  does.
- **When checkout needs more:** add a `WriteBatch` port. It collects
  `insert`/`update(expected_version)` operations and commits them atomically: one
  Postgres transaction, or one D1 `batch()`.
- It is *not* a generic `UnitOfWork` that holds a transaction open. That wouldn't work
  on D1.

### 3.8 Crate and tooling

- New crate `crates/hypha-store-postgres`, in the hosts layer (Tokio, not WASM). It
  depends on the domain crates and sqlx, and implements every repo trait on
  `PgStore { pool }`. Migrations live in `migrations/` and run with `sqlx::migrate!`.
- `hypha-host-axum` picks its storage at startup: `DATABASE_URL` set means Postgres,
  otherwise in memory.
- **Decided: D4, sqlx query style.** *Superseded by section 7: `tokio-postgres`.*
  - **(a) Runtime `query_as`.** No database needed at build time. Mistakes surface in
    the contract tests.
  - **(b) Compile-time `query!` macros**, with an offline `.sqlx` cache committed via
    `cargo sqlx prepare`. SQL is checked against the schema at compile time, but it adds
    a tool to install and a step to remember.
  - *Recommendation: (a) to start.* Move to (b) once the schema settles.
- Local Postgres through a `compose.yaml` (Docker Desktop on Windows). **Deferred: D5.**
  Settled in step 5. The default is Docker unless Danilo prefers a native install.

## 4. Order of work (each step ends green: fmt, clippy, tests, WASM, e2e)

1. **D3 (a):** move catalog products and fixed prices behind narrow port methods,
   still in memory. **Done.** `CatalogRepo::{add_products, remove_products,
   included_products}` and `PriceListRepo::{set_fixed_prices, remove_fixed_prices,
   fixed_prices}` take and return batches. Pricing gets them through a plain-data
   `PricingContext`. Pricing a cart costs a fixed 5 queries, plus one per product until
   `get_many` in step 2.
2. Add versions, snapshots and `restore`, the `insert`/`update` split and the typed
   `RepoError`, still in memory. Commands map `Duplicate`/`StaleVersion` to domain
   errors and codes. Add the D6 money cap, and `ProductRepo::get_many` so a cart's
   products load in one query. **Done.** `Version` + `Versioned` in core; the testkit `Table` enforces versions
   and unique keys; `create_simple_product`, `create_market` and `create_catalog` map a
   racing `Duplicate` to the same error as their pre-check (tested with repos that hide rows
   from the pre-check). Every root has `snapshot()` / `restore()`. Pricing a cart costs 6
   queries whatever its size.
3. Contract tests in testkit. The in-memory repos pass them. **Done.** `hypha_testkit::contract`, one
   `async fn` per repo trait taking an `AsyncFn` factory (any runtime). Checked by injecting
   three bugs into the in-memory repos (no version check, no unique keys, a no-op remove):
   each was caught.
4. **D1 experiment (throwaway, not merged).** A minimal `workers-rs` Worker with one table,
   run with `wrangler dev`. It verifies:
   - `batch()` is atomic through `workers-rs`;
   - a version mismatch can **abort** a batch, not just match zero rows. In a batch, a
     zero-row `UPDATE … WHERE version = ?` raises no error, so the statements after it
     (replacing child rows) would still run. Candidate fix:
     `SET version = CASE WHEN version = ?2 THEN version + 1 END` with
     `version NOT NULL`, so a mismatch fails the constraint and rolls the batch back;
   - what a UNIQUE violation looks like by the time it reaches Rust. SQLite reports
     `table.column`, not a constraint name, so D2 keys are mapped from that;
   - `meta.changes` is available per statement.

   The Postgres adapter is then written in the same shape: each save is one atomic write
   with no reads inside, and no Postgres-only features.

   **Done (2026-10-12)**, with `worker` 0.8.7, `wrangler` 4 and local D1. Results are in
   section 6.
5. `hypha-store-postgres`: schema, migrations, the repo impls. The contract tests pass
   against Postgres. **Done (2026-10-13)** on `tokio-postgres` (section 7): all five
   contracts pass against Docker Postgres 18.6, and injected bugs (missing version checks,
   missing key mappings) make them fail. It also builds for `wasm32`. Batched writes use
   `unnest` (`WITH ORDINALITY` to keep order). Product children are still inserted row by
   row; batch them if large products make saves slow. `uuid[]` for variant selections is
   Postgres-only, which is acceptable now that D1 is optional.
6. The host chooses storage from `DATABASE_URL`. `scripts/e2e.sh` passes against
   Postgres.
7. Later, in its own note: the Cloudflare host and the D1 adapter.

Steps 1–3 involve no database at all, and they're where most of the design risk is.

## 5. Not decided here

- Retrying on `StaleVersion`. *Proposal:* return 409 and let the client retry. Revisit
  whether commands should retry internally once there's real traffic.
- Exposing versions over HTTP (`ETag` / `If-Match`) so admin edits can't overwrite each
  other. Useful for the admin panel later.
- Soft deletes versus hard deletes, and audit history.
- Stock and inventory, which bring their own concurrency rules.
- **Multi-tenancy:** shared tables with a `store_id` column, or one database per store.
  D1's 10 GB cap and 50k databases per account point towards one database per store,
  at least on Cloudflare. It needs its own note before customers and orders arrive.
- **Durable Objects SQLite** for coordination within a single object (for example a
  per-store sequence of order numbers). D1 docs say its limits and pricing are identical
  to D1. Not a replacement for the storage ports.

## 6. D1 experiment results (step 4)

Setup: a throwaway Worker (`worker` 0.8.7 with the `d1` feature), run with
`wrangler dev --local`. That's local D1, on the same SQLite engine as production. Error
text may differ slightly in production, so the adapter should match on the stable parts
quoted below. Each experiment ran on a freshly created schema.

| # | Experiment | Observed |
| --- | --- | --- |
| 1 | `batch()` whose 2nd insert breaks a UNIQUE | Error, and **the 1st insert was rolled back** (0 rows). Batches are atomic through `workers-rs`. |
| 2 | UNIQUE violation, as seen in Rust | `UNIQUE constraint failed: products.handle: SQLITE_CONSTRAINT (extended: SQLITE_CONSTRAINT_UNIQUE)`. For the primary key: `…products.id … SQLITE_CONSTRAINT_PRIMARYKEY`. The **constraint name is not reported**, even when the constraint is declared as `CONSTRAINT product_handle UNIQUE (handle)`. Only `table.column` is. |
| 3 | Save batch with a plain `UPDATE … WHERE version = 99` (stored version 1), then delete and insert lines | **No error.** `meta.changes` = `[0, 1, 1]`. The version stayed 1, but **the lines were replaced**. The trap is real: someone else's cart would be silently overwritten. |
| 4a | Same batch with `SET version = CASE WHEN version = ?2 THEN version + 1 END` (wrong version) | Error `NOT NULL constraint failed: carts.version`. **The whole batch rolled back**, so the lines are untouched. The fix works. |
| 4b | Same, with the right version | No error, `changes` = `[1, 1, 1]`, version 2, lines replaced. A normal save. |
| 5 | Save of a cart that doesn't exist | With new lines: `FOREIGN KEY constraint failed` (no table named) and nothing written; **foreign keys are enforced by default**. Without new lines: no error, `changes` = `[0, 0]`, nothing written. |
| 6 | Read back 2⁵³ − 1 and 2⁵³ + 1 | 2⁵³ − 1 is exact. 2⁵³ + 1 came back as **9007199254740992, silently wrong** (`CAST(v AS TEXT)` shows the true value). This confirms D6. |

Also found: **`impl Display for D1Error` in `worker` 0.8.7 panics** when the JS error's
`cause` isn't a string. The panic hung the request (the runtime reported it as hung, not
as an error). Adapters must not format a `D1Error` with `{}`, and must not call
`worker::Error::to_string()` on one. Use `D1Error::cause()` and the inner
`js_sys::Error::message()`.

### Rules for the D1 adapter (and the shape the Postgres adapter follows)

- **Every `update` is one `batch()`** whose first statement is the version step:
  `UPDATE <root> SET version = CASE WHEN version = ?expected THEN version + 1 END, … WHERE id = ?id`,
  with `version INTEGER NOT NULL`. The child-row statements follow it.
- **Map the errors** (on `cause()`):
  - `NOT NULL constraint failed: <root>.version` → `StaleVersion`
  - `FOREIGN KEY constraint failed` inside a save batch → `StaleVersion` (the root is gone)
  - `UNIQUE constraint failed: <table>.<column(s)>` → `Duplicate(key)`, using a fixed
    table of `"products.handle" → keys::HANDLE`, `"products.id" → keys::PRODUCT_ID`,
    and so on. The contract tests check each key.
  - anything else → `Backend`
- **After a successful batch, check that the version statement reported
  `meta.changes == 1`.** If it's 0, the root row didn't exist and nothing was written
  (experiment 5 without lines), so return `StaleVersion`.
- **Inserts** are one batch too: the root (with `version = 1`), then the children.
- **Postgres** follows the same shape inside one short transaction: the same
  `CASE … END` / `NOT NULL` trick, or `WHERE version = $2` plus a check of the affected
  row count before touching the children. It reads nothing inside the transaction and uses
  no Postgres-only features. Unique keys are mapped from constraint names, which Postgres
  does report (`products_handle_key`).

## 7. Postgres from Cloudflare Workers through Hyperdrive (2026-10-13)

**Question:** can the Postgres adapter run on Cloudflare too, so one SQL adapter serves
every host? **Answer: yes, verified locally.**

The pieces:
- `worker` 0.8.7 has a `tokio-postgres` feature, and `Env::hyperdrive("…")` returns a
  binding with `connect() -> Socket` and `connection_string()`.
- `tokio_postgres::Config::connect_raw(socket, worker::postgres_tls::PassthroughTls)`
  opens a `Client` on that socket. `tokio-postgres` is built with its `js` feature for
  WASM, and the `Connection` future is driven with `wasm_bindgen_futures::spawn_local`.
- `tokio-postgres` documents `query_typed` as being for "environments where prepared
  statements aren't supported (such as Cloudflare Workers with Hyperdrive)". It sends
  each query without a named prepared statement, so it has no prepare and close round
  trips.

Experiment: a throwaway Worker (`D:\dev\hypha-pg-spike`, not merged) running under
`wrangler dev --local`. Its Hyperdrive binding's `localConnectionString` pointed at the
Docker Postgres 18.6 from `compose.yaml`.

| # | Experiment | Observed |
| --- | --- | --- |
| 0–1 | Connect through the binding; `query_typed("SELECT version()")` | Works. Postgres 18.6 answered over the Hyperdrive socket. |
| 2 | Unique violations | Structured errors: SQLSTATE `23505`, `constraint = "product_handle"` (our declared name) or `"products_pkey"`, plus `table`. **No text parsing needed**, unlike D1. |
| 3 | Interactive transaction: `UPDATE … WHERE version = $2 RETURNING version`, then replace the lines only if a row came back | Stale (expected 99): no row, rolled back, lines untouched. Good (expected 1): committed, version 2, lines replaced. `BEGIN`/`COMMIT`/`ROLLBACK` work. |
| 4 | The D1-shaped `CASE … END` / `NOT NULL` trick | SQLSTATE `23502` on `carts.version`. The transaction was rolled back and nothing changed. The same SQL shape works on both backends. |
| 5 | `i64` round trip of 2⁵³ + 1 | **Exact.** The binary protocol never passes through a JavaScript number, so the precision problem exists only on D1. The money cap (D6) still matters for JSON clients and D1. |
| 6 | A named prepared statement (`client.query`) | Works locally, but local Hyperdrive doesn't pool, so this **says nothing about production**. Rule: use `query_typed` only. |

### Consequences for the plan

- **One SQL adapter for all hosts.** `hypha-store-postgres` uses `tokio-postgres` and
  builds for native targets (axum on a VPS, Lambda) **and** for `wasm32-unknown-unknown`
  (Workers + Hyperdrive). It never owns a runtime or a pool: it receives a `Client`.
  - The axum host pools connections (for example `deadpool-postgres`) and lends a
    `Client` per request.
  - The Workers host opens one connection per request through the Hyperdrive binding.
    Hyperdrive pools on Cloudflare's side; in-Worker pools don't work, since Workers keep
    no state between requests.
- **Adapter rules:**
  - Only `query_typed*` and `batch_execute`, never `query`, `prepare` or `execute`.
  - Map errors by SQLSTATE and constraint name:
    - `23505` + constraint name → `Duplicate(key)`, using a fixed table, for example
      `product_handle → keys::HANDLE`
    - `23502` on `<root>.version` → `StaleVersion`
    - `23503` inside a save → `StaleVersion`
    - anything else → `Backend`
  - Keep the D1 save shape: the version step first, then the children, in one short
    transaction with no reads in it.
- **Hyperdrive's query cache must be off for this database** (`--caching-disabled`). It
  caches reads for up to 60 s and isn't invalidated by writes, so a command would load old
  versions and hit avoidable `StaleVersion` conflicts. A separate, cached Hyperdrive
  config can be added later for storefront reads that don't need to be fresh.
- **Migrations:** sqlx's migrator is gone. A small runner with `include_str!`'d SQL files
  and a `schema_migrations` table, run from native code (a CLI or host startup), never
  from a Worker.
- **D1 becomes optional.** It's still viable (section 6), but no longer needed for the
  Cloudflare host. Revisit it for very cheap per-store databases.
- **Still unverified:** production Hyperdrive (transaction pooling, real TLS, latency),
  because it needs a Cloudflare account. Test it before relying on the Workers host.
