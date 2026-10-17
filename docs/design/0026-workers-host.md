# 0026: The API on Cloudflare Workers

- **Status:** Proposed (2026-10-17); W3 and W5 accepted. A spike (§8) settles the open
  facts before the refactor starts. Which database to recommend is open (§9).
- **Scope:**
  - `hypha-host-workers`: the same routes, commands and Postgres adapter as the VPS,
    on Workers, through Hyperdrive;
  - what replaces each host-only piece of `hypha-host-axum` (pool, jobs, background
    work, rate limits, time, IDs, password hashing, outbound HTTP);
  - splitting the HTTP layer out of `hypha-host-axum` so both hosts share it;
  - `hypha deploy --target cloudflare` (0025): one command for the API, the admin and
    the storefront.
- **Not in scope:**
  - the admin's and storefront's Workers caching adapters (0025, §6: KV and the Workers
    Cache), beyond what the deploy configures;
  - building a D1 adapter (§9 weighs it; a note of its own if it goes ahead);
  - multi-tenancy (0006 stays model A: one store per deployment).
- **Related:** 0001 (Postgres, the `!Send` rule), 0006 (tenancy), 0009 (rate limits),
  0013 (media), 0016 (outbox, jobs), 0017 (configuration), 0024 (caching), 0025
  (hosting, measurements).

## Why

A store on a domain, `hypha deploy`, click through the setup wizard, sell: for $5 a
month or close to it. Cloudflare runs the admin and the storefront already (vinext);
the API is the missing piece. 0025 measured what the API costs per request on a VPS,
and cut most of it; this note carries that host to Workers.

## Decisions

| | Decision |
| --- | --- |
| W1 | **A native Workers host, `wasm32-unknown-unknown` with workers-rs** (the `worker` crate, its `http` feature), not Cloudflare Containers (0025, H2) and not, yet, workers-rs's Emscripten target. Emscripten would run Tokio and real sockets nearly unchanged, but it's an experimental preview on a patched Tokio fork; revisit when it isn't. *Proposed* |
| W2 | **The same axum routes.** The `http` feature hands axum an `http::Request` and takes its response, so the route table, extractors, error mapping, DTOs and OpenAPI documents are shared, not rewritten. *Proposed* |
| W3 | **Split `hypha-host-axum` into a library and two hosts.** `hypha-http` (routes, extractors, errors, DTOs, OpenAPI, the `Storage` and host-port traits, the key cache) builds for wasm; `hypha-host-axum` keeps Tokio, the pool, the job runner and signals; `hypha-host-workers` is the Workers `cdylib`. A pure move first, in its own commit (CLAUDE.md). *Hard to reverse: crate layout; Accepted* |
| W4 | **Postgres through Hyperdrive with the same adapter.** `env.hyperdrive("DB")?.connect()` gives a `worker::Socket` (Tokio's `AsyncRead`/`AsyncWrite`), which `tokio_postgres::Config::connect_raw` takes; the connection task runs with `spawn_local`. One connection per request; Hyperdrive pools. No statement cache (0025, §6: `database.statement_cache` is a VPS setting; Hyperdrive's transaction pooling and prepared statements are a spike question). Cloudflare lists rust-postgres as supported and recommends `query_typed`, which every repository already uses. *Proposed* |
| W5 | **Workers Paid ($5/month) is the baseline.** The free plan's 10 ms of CPU per request can't fit a staff login (Argon2id at OWASP's 19 MiB, 2 passes, natively tens of milliseconds, more in wasm), nor a dynamic page render (0025: ~13–30 ms). Free stays possible for a storefront that serves cached pages, never for the admin. Lowering Argon2's cost to fit is not an option. *Accepted* |
| W6 | **Jobs are Cron Triggers, woken by writes.** `#[event(scheduled)]` runs the same steps the VPS runner does (`dispatch`, `search_sync`, `prune`, `media_sweep`); a successful write also runs one dispatch step in `ctx.wait_until` (the Workers form of 0025's `wake_after_writes`). Cron's floor is a minute. *Proposed* |
| W7 | **A quiet store doesn't touch the database.** A cron tick that queried Postgres every minute would keep a serverless database (Neon) awake for good. Writes set a `dirty` marker in KV (and retries record when they're next due); a tick reads KV first and runs the steps only when there's work or a retry is due. *Proposed* |
| W8 | **Rate limits on Cloudflare's Rate Limiting binding** (`RateLimiter`) behind 0009's `RateLimiter` port: per location and approximate, which suits brute-force limits on login and password reset. A Durable Object would be exact; not worth it for these routes. *Proposed* |
| W9 | **Migrations and first-start setup run from the CLI, not the Worker.** There's no startup hook, and advisory locks don't survive transaction pooling. `hypha deploy` runs the host binary's `setup` (migrations, the owner, the default location, the search index) against the database's direct URL before deploying the Worker. *Proposed* |
| W10 | **Files on R2 through the S3 adapter we have.** `hypha-store-s3` already takes its HTTP through an `HttpClient` port; the Workers host implements it with `worker::Fetch`. Public files are served from the bucket's custom domain, thumbnails by Cloudflare's image transformations (`media.transforms = "cloudflare"`, 0013). *Proposed* |
| W11 | **Configuration from Worker variables.** `hypha-config` parses a string and reads the environment through a function it's given (0017, CF3): the Worker passes `HYPHA_CONFIG` (the TOML, if any) and reads `env.var` / `env.secret`. Same keys, same checks. *Proposed* |

## 1. What changes, piece by piece

| In `hypha-host-axum` today | On Workers | Notes |
| --- | --- | --- |
| `PgStorage`: deadpool pool, statement cache | Hyperdrive socket per request, `connect_raw` | Request-scoped; no cache (W4) |
| `axum::serve`, Tokio runtime, signals | `#[event(fetch)]` calling the router | Shared routes (W2) |
| `route_table!` per concrete storage | The same macro for the Workers storage | Proves `Send` the same way |
| `jobs.rs` runner, `wake_after_writes` | `#[event(scheduled)]` + `ctx.wait_until` + KV marker | W6, W7 |
| `Background` (`tokio::spawn`) | `ctx.wait_until` | Password-reset mail, webhooks after commit |
| `MemoryRateLimiter` (`governor`, `Instant`) | `RateLimiter` binding | W8; `Instant::now` panics on this target |
| `SystemClock` (`SystemTime::now`) | `Date.now()` (`worker::Date`) | Panics on `wasm32-unknown-unknown` |
| `UuidV7Ids` | UUIDv7 from `Date.now()` + `getrandom` (`wasm_js`) | Same IDs, ordered |
| `HostEnv` randomness | `getrandom` → `crypto.getRandomValues` | |
| `HostEnv` Argon2 on blocking threads | Argon2 inline | W5; CPU per login measured in the spike |
| `SecretBox` (AES-GCM, `HYPHA_SECRET_KEY`) | Unchanged (pure Rust), key from a secret | |
| `reqwest` (rate apps, webhooks) | `worker::Fetch` behind `WebhookSender` / the rates port | `reqwest`'s wasm futures aren't `Send` |
| `outbound.rs` private-address check (DNS) | Not needed: a Worker's `fetch` can't reach private networks | Keep the URL checks |
| `KeyCache`, the admin's own cache | Unchanged, per isolate | Isolates are reused; correct either way |
| Startup: migrations, owner, location, search | `hypha … setup` from the CLI | W9 |
| Logging (`tracing` subscriber) | `console` (`worker::console_log!`), Workers Logs | |
| Emails logged | Logged; an email provider is 0008 §9 | |

### `Send`

Axum needs handler futures to be `Send`. On the Workers storage they mostly are: the
`tokio_postgres::Client` talks to its connection task through channels, and the socket
(the `!Send` part) lives only in that task. What isn't (`worker::Fetch`, bindings in
the state) is wrapped with workers-rs's `send` module (`SendFuture`, `SendWrapper`):
Workers are single-threaded, which is what makes that sound. 0001's rule stays: ports
have no `Send` bound; hosts prove it for their concrete types.

## 2. Requests and their cost

On Workers, every API call and every page render is an invocation billed by count and
CPU time, and every query is a round trip through Hyperdrive. 0025 cut both on the VPS
(pipelined loads, one market read, cached publishable keys, batch endpoints for the
admin) and that carries over. What this host adds:

- **Storefront → API over a Service Binding**, not the public internet: no TLS or DNS,
  and the API never sees shopper traffic directly. (Whether binding calls are billed
  as requests: a spike question.)
- **Static pages from the edge.** The storefront's product, category and home pages
  come from the Workers Cache adapter, invalidated by the webhook (0024, §7): a page
  view that hits it invokes nothing.
- **Search and cart pages cached by URL** for a minute (0025, §6): they hold nothing
  about the shopper.

A rough month for a small store (1,000 visitors a day, five pages each): ~150,000 page
views, most from cache; well under a million invocations and 10 million CPU-ms, inside
the paid plan's included 10 million requests and 30 million CPU-ms. Hyperdrive's free
allowance is 100,000 queries a day, enough for this store; the paid plan has no limit.

## 3. Limits that shape the design

From Cloudflare's limits page (October 2026):

| | Free | Paid |
| --- | --- | --- |
| CPU per request | 10 ms | 30 s by default, up to 5 min |
| Memory per isolate | 128 MB | 128 MB |
| Worker size, uncompressed | 64 MiB | 64 MiB |
| Subrequests per invocation | 50 | 10,000 |
| Simultaneous outgoing connections | 6 | 6 |
| Startup time | 1 s | 1 s |
| Requests per day | 100,000 | No limit |
| Cron Triggers per account | 5 | 250 |

- **Startup time (1 s)** is the one to watch: the module holds axum, every domain crate
  and utoipa. The spike measures it; if it's tight, the OpenAPI documents become static
  files instead of being built at runtime.
- **Six simultaneous connections**: a request holds one Postgres socket, and the
  webhook fan-out sends deliveries one after another, as it does now.
- **128 MB per isolate**: Argon2id takes 19 MiB per hash; one login at a time per
  isolate is fine.

## 4. Jobs

| Job | Trigger | Notes |
| --- | --- | --- |
| Dispatch (fan out, deliver) | Every minute, if the KV marker says so; one step after each write (`wait_until`) | Retries record their next due time in KV |
| Search sync | After a dispatch step that ran handlers; every minute if marked | The same `SearchQueue` |
| Prune | Hourly | Unchanged |
| Media sweep | Hourly | Unchanged |

Each is a step from a domain crate that says whether there's more (0016, §7), so the
Workers host calls the same functions the VPS runner does; a step with more to do runs
again within the tick, up to a time budget.

## 5. Deploy

`hypha init --target cloudflare` asks for a Postgres URL (Neon, Supabase, or any) and
the domains; `hypha deploy` then:

1. Runs `setup` against the direct URL (W9).
2. Creates the Hyperdrive configuration, the R2 bucket (and its public domain), the KV
   namespace and the rate limiter, through Cloudflare's API (`wrangler` or `cf`).
3. Sets the secrets (`HYPHA_SECRET_KEY`, the S3 keys, the owner's first password).
4. Builds and deploys three Workers: the API (`worker-build --release`), the admin and
   the storefront (`@vinext/cloudflare deploy`), with the storefront and admin bound to
   the API by Service Bindings.
5. Connects the storefront (a publishable key, its webhook), as on a VPS (0025, §2).

## 6. Order of work

1. **Spike (§8)**: go or no-go, and the numbers for this note.
2. **Move**: `hypha-http` out of `hypha-host-axum` (W3), no behaviour change; the VPS
   host keeps passing its tests and e2e.
3. **Host ports**: clock, IDs, randomness, Argon2, `WebhookSender`, the S3 `HttpClient`,
   the rate limiter, background work: each a trait the two hosts implement.
4. **`hypha-host-workers`**: fetch, the Hyperdrive storage, configuration; the route
   tests' scenarios run against it in `wrangler dev`.
5. **Jobs** (W6, W7).
6. **CLI**: `--target cloudflare` (§5).
7. **Caching**: the storefront's and admin's adapters (0025, §6), and the e2e script
   against a deployed Worker.

## 7. Risks

- **Hyperdrive and `tokio-postgres` over `connect_raw`** isn't a path Cloudflare
  documents for Rust; the `worker` crate carries `tokio-postgres` as an optional
  dependency, which suggests it's meant to work. The spike proves it first.
- **Startup time and module size** with everything in one Worker (§3).
- **Argon2 CPU in wasm**: if a login costs seconds, staff logins would need a different
  approach (an Argon2 implementation tuned for wasm, a WebCrypto KDF for new hashes with
  a migration of old ones).
- **workers-rs churn**: 0.x, with the axum example marked untested; versions pinned
  exactly, as with Astryx.

## 8. Spike

One Worker, a day or two, before any refactor:

- `GET /health` and `GET /storefront/products/by-handle/{handle}` through the existing
  axum handler, on a Workers storage (Hyperdrive, `connect_raw`, `PgStore`), against
  the test server's database or a Neon one.
- Measure: wasm size (raw and gzip), cold start, CPU per request (product read, search),
  query latency through Hyperdrive, and one Argon2id hash's CPU time.
- Settle: whether a Service Binding call counts as a request; whether Hyperdrive keeps
  prepared statements (and whether a request-scoped cache helps); that `wait_until` and
  `scheduled` behave as §4 needs; the Rate Limiting binding on the plans.

Go if a product read stays within a few milliseconds of CPU, a cold start under the
limit with room, and a login under a second of CPU.

## 9. Databases (open)

The first Workers host uses Postgres through Hyperdrive (W4): the adapter, its contract
and its tests are the ones the VPS runs, so it works with any Postgres (Neon, Supabase,
RDS, the user's own). Which provider `hypha init` suggests is left open: providers may
sponsor or partner with the project.

**D1** would make a store entirely Cloudflare's (no outside account, no Hyperdrive, one
bill): attractive for "a domain and `hypha deploy`". It's SQLite, so it means a second
storage adapter, `hypha-store-d1`, next to `hypha-store-postgres`, not a setting. The
architecture is built for that: every repository is a port, the `hypha_testkit::contract`
suite defines what an adapter must do (round trips, versions, unique keys, batches), and
the in-memory adapter already proves the domain doesn't lean on Postgres. What it would
take:

| Postgres feature the adapter uses | On D1 |
| --- | --- |
| Transactions (`in_transaction!`, `PgBatch::commit`) | No interactive transactions; a `batch()` runs its statements in order and rolls back on any failure. Hypha's writes already are "read, check, then commit one batch" (0001), so they map onto it. |
| Version checks (`UPDATE … WHERE version = $n`, `StaleVersion`) | A batch can't branch on rows affected; an assertion row aborts it instead (e.g. inserting `changes() = 1` into a table with `CHECK (ok)`), which the contract's stale-version scenarios would hold it to. |
| Snapshot reads (`consistent_read!`) | A read batch is one transaction: the pipelined loads (0025, §6) become one `batch()` each. |
| Arrays (`= ANY($1)`, `uuid[]` columns) | No arrays; at most 100 bound parameters per query. Lists go in as one JSON parameter through `json_each`; array columns become JSON or child tables. |
| `FOR UPDATE SKIP LOCKED` (outbox, delivery and search leases) | One writer per database: a lease is a plain `UPDATE … RETURNING`. |
| Full-text search (the built-in engine, 0019) | FTS5, reportedly available on D1; the `search_index` contract holds ranking to `reference::search`. |
| `uuid`, `timestamptz`, `jsonb`, `bytea` | Blobs or text, integers (µs, as `Timestamp` already is), JSON text. |

Limits that matter for a store: 10 GB per database on the paid plan (500 MB free), 1,000
queries per invocation (50 free), 100 parameters and 100 KB per statement. Included in
Workers Paid: 25 billion rows read and 50 million written a month, 5 GB stored; on the
free plan, 5 million rows read and 100,000 written a day.

A D1 adapter is the size of the Postgres one (about 10,000 lines) plus its migrations;
nothing above the adapter changes. The order would be: Postgres through Hyperdrive first
(it reuses everything), then D1 as its own note and adapter, starting with a spike on
the two risky rows (version checks in a batch, FTS5 ranking against the contract).

## Sources

- [workers-rs README](https://github.com/cloudflare/workers-rs) (the `http` feature,
  axum, Durable Objects, the Emscripten preview); [`worker` crate
  docs](https://docs.rs/worker/latest/worker/) (`Socket`, `Hyperdrive`, the `send`
  module, `RateLimiter`, `ScheduledEvent`).
- [Workers limits](https://developers.cloudflare.com/workers/platform/limits/).
- [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/), [D1
  limits](https://developers.cloudflare.com/d1/platform/limits/); D1's batch semantics
  (atomic, no interactive transactions) from [event-driven.io](https://event-driven.io/en/cloudflare_d1_transactions_and_tradeoffs/),
  to confirm against Cloudflare's docs in the spike.
- [Hyperdrive: connect to Postgres](https://developers.cloudflare.com/hyperdrive/configuration/connect-to-postgres/)
  (rust-postgres supported, `query_typed`); [Hyperdrive
  pricing](https://developers.cloudflare.com/hyperdrive/platform/pricing) (100,000
  queries a day on the free plan).
