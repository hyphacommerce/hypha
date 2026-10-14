# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Hypha (working name): open-source, modular e-commerce API in Rust (edition 2024, Cargo workspace, `crates/*`).
- **Modular**: one crate per domain; behavior is extended through plugins ("spores") via traits and events, not by editing core.
- **Agent-friendly, in two specific ways** (shopping agents are *not* the target of the core API):
  1. **Building Hypha**: the codebase is meant to be easy for coding agents to extend: core, domain crates and plugins ("spores"). Favor explicit types, small crates with clear ports, exact errors and fast deterministic tests over cleverness.
  2. **Admin assistance**: agents will help merchants operate the system from the admin panel. Every operation is a command with typed input, required scopes and typed errors, so the same surface can be exposed over HTTP, MCP and CLI; such agents act as `Actor::Agent` with their own scopes.
  - Agentic *shopping* goes through a separate adapter for the Universal Commerce Protocol (UCP, ucp.dev; planned via ucphub.ai), not through the core commands directly. Design storefront-facing read models (e.g. priced carts) so they map cleanly onto UCP, e.g. per-line problems → UCP `messages` with a `path` to the line.
- **Large catalogs**: sparse explicit variants (never generate the Cartesian product of options); Shopify-style markets / catalogs / price lists.
- **Multi-host**: the same core runs on Cloudflare Workers (WASM), AWS Lambda, Vercel and a plain VPS (axum).
- It's also a Rust learning project for the maintainer (Danilo, TypeScript background). See "Working with Danilo".

## Commands

Run all four before considering work done:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p hypha-core -p hypha-products -p hypha-markets -p hypha-pricing -p hypha-cart -p hypha-tax -p hypha-inventory -p hypha-customers -p hypha-stores -p hypha-orders -p hypha-checkout -p hypha-auth -p hypha-users -p hypha-store-postgres --target wasm32-unknown-unknown
```

Extend the wasm build list when a domain crate or storage adapter is added.

- Single crate's tests: `cargo test -p hypha-products`
- Single test by name: `cargo test -p hypha-products rejects_same_handle_products`
- Only an integration test file: `cargo test -p hypha-products --test commands`
- Run the HTTP host (binds `HYPHA_BIND`, else `0.0.0.0:$PORT`, else `127.0.0.1:3000`; `RUST_LOG` respected; Ctrl+C or SIGTERM drains in-flight requests; `GET /health` for probes): `cargo run -p hypha-host-axum` (in memory), or with `DATABASE_URL=postgres://hypha:hypha@localhost:5432/hypha` for Postgres (migrations run on startup; also needs `HYPHA_SECRET_KEY`, 64 hex characters from `openssl rand -hex 32`, kept across restarts since it seals 2FA secrets). Every route but `/health` and the login ones needs credentials (design note 0008): on first start, `HYPHA_OWNER_EMAIL` and `HYPHA_OWNER_PASSWORD` create the owner, who logs in with `POST /auth/login`. Emails (invites, resets) are logged, not sent; `HYPHA_ADMIN_URL` turns their tokens into links.
- End-to-end check against a freshly started host (Git Bash on Windows), started with `HYPHA_OWNER_EMAIL=owner@example.com HYPHA_OWNER_PASSWORD=e2e-owner-password` (the script logs in as that owner): `bash scripts/e2e.sh` (`HYPHA_URL` for another port). Asserts status + error `code` per step, exits non-zero on failure. Run it after changing routes, DTOs or error mapping, and add a step for new routes. Against Postgres, use a fresh database (the script reuses handles and countries): `docker compose exec -T postgres psql -U hypha -c "DROP DATABASE IF EXISTS hypha_e2e" -c "CREATE DATABASE hypha_e2e"`, then start the host with `DATABASE_URL=.../hypha_e2e` and a `HYPHA_SECRET_KEY`.
- Postgres (Docker Desktop): `docker compose up -d`, then `DATABASE_URL=postgres://hypha:hypha@localhost:5432/hypha cargo test -p hypha-store-postgres` runs the repository contract against it (each test gets its own schema). Without `DATABASE_URL` those tests skip themselves.

## Architecture (hexagonal / ports and adapters)

Dependencies point inward: hosts → domain crates → core. Core and domain crates never depend on storage, HTTP or runtimes.

Domain crates are layered; a crate may only depend on layers below it (Cargo rejects cycles, so a wrong edge forces a painful restructure later):

| Layer | Crates | May depend on |
| --- | --- | --- |
| Hosts | `hypha-host-axum` | anything |
| Storage adapters | `hypha-store-postgres` (tokio-postgres; native and Workers via Hyperdrive) | domain crates, core; never hosts |
| Workflows | `hypha-checkout` (cart → order) | composing and leaf domains, core |
| Composing domains | `hypha-cart`, `hypha-users` | leaf domains, core (`hypha-users` → `hypha-auth` only) |
| Leaf domains | `hypha-products`, `hypha-markets`, `hypha-pricing`, `hypha-tax`, `hypha-inventory`, `hypha-customers`, `hypha-stores`, `hypha-orders`, `hypha-auth` | core; `hypha-pricing` → `hypha-markets` only |
| Core | `hypha-core` | nothing internal |

Leaf functions take the plain values they need (IDs, `Price`), not another leaf's aggregate. E.g. `resolve_price` takes `ProductId`, `VariantId` and the base `Price` rather than `&Product`; the composing command (cart) loads the product and passes them in. Ask before adding a new edge between domain crates.

- `hypha-core`: shared types only — `Money`/`Price`/`BasisPoints` (`money/`), prefixed IDs (`ids.rs`), `CountryCode`, `Email`, `Address`, `Scope`, `Ctx`/`Actor`, generic ports (`IdGenerator`, `Clock`, `HasBatch`, `RepoError` in `ports.rs`), `Timestamp` (`time.rs`, µs since the epoch), idempotency keys (`idempotency.rs`: a keyed command checks `earlier_result` first, then stages an `IdempotencyRecord` in the same batch as its write, and after *any* failed keyed write looks the key up again and replays the winner, since a racing request with the same key can fail it on the cart or order before the key; see `hypha-cart`'s `write_cart` and `hypha-checkout`'s retry loop). No domain logic, no I/O.
- Domain crates (`hypha-products`, `hypha-markets`, `hypha-pricing`, `hypha-tax`, `hypha-inventory`, `hypha-customers`, `hypha-orders`, `hypha-cart`, and the `hypha-checkout` workflow): aggregates with private fields and validated constructors, a repository trait (port) per aggregate in `repo.rs`, error enums in `error.rs` (a crate's `CommandError` sits in `commands.rs`), and commands in `commands.rs` (`hypha-products` is the simplest template; `hypha-cart` shows a command that composes several domains; `hypha-checkout` writes several aggregates in one batch and retries when it loses a race).
- `hypha-auth` and `hypha-users` (design note 0008): who is calling. `hypha-auth` holds identities with typed credentials (password, TOTP, recovery codes; stored as kind + lookup + JSON so a new login method needs no column), server-side sessions (a pending state while a second factor is owed), API keys, password resets and prefixed tokens stored only as SHA-256; hashing and sealing are ports (`PasswordHasher`, `SecretBox`), and its commands take one `env: &impl AuthEnv` bundling the non-storage ports. `hypha-users` holds roles (named scope sets; the owner role has every scope), users and invites, and the staff flows (`login`, `accept_invite`, `authenticate_staff`). Nobody grants a scope they lack (`require_grantable`); a write scope implies its read scope (`Scope::implied_by`, applied in `Ctx::has_scope`).
- `hypha-testkit`: in-memory repos for every port (`InMemoryProductRepo`, `InMemoryCartRepo`, …) and deterministic ports (`SequentialIds`, `FixedClock`). It depends on domain crates, so domain crates use it only as a dev-dependency from their `tests/` folder.
- `hypha-host-axum`: the only place with Tokio, HTTP, auth parsing, logging subscribers (`tracing`) and real ID generation (`UuidV7Ids`). `routes/` holds one module per area with its own request/response DTOs (never serialize domain types directly); `error.rs` maps every crate's `CommandError` to a status (403 scope, 404 resource in the URL, 409 conflict or the state of the resource in the URL, 422 invalid input incl. unknown IDs in the body (except the five routes listed at the top of `error.rs` that store other domains' IDs unchecked), 500 storage with the message hidden) and fills `details` for errors that carry data a client acts on (shortages, unavailable lines, totals); `extract.rs` wraps `Json`/`Path` so even axum's rejections return `{"code": …, "error": …, "details": …}`; routes whose command takes an idempotency key use `KeyedJson` (reads the `Idempotency-Key` header, fingerprints the request) and `idempotent_response`. Request DTOs are `#[serde(deny_unknown_fields)]`; PATCH bodies follow JSON Merge Patch (absent keeps, `null` clears) via the `patch::nullable` / `patch::non_null` field helpers. Storage is chosen at startup (`state.rs`): `MemoryStorage` (testkit's `InMemoryPorts`) or `PgStorage` (deadpool pool; one `PgStore` per request) behind the `Storage` trait; handlers are generic over it and call `state.storage.ports()` once per request. Routes are listed once in the `router!` macro in `routes/mod.rs`, stamped out per concrete storage, because axum can only prove handler futures `Send` for concrete types (add new routes there). Every handler takes a `Caller` (`extract.rs`), the one place a request becomes a `Ctx`: the extractor only reads `Authorization: Bearer` (a staff session `hypha_sess_…`, the role's scopes read fresh each request, or an API key `hypha_sk_…` / `hypha_pk_…`) or `X-Publishable-Key` (none is 401 `unauthenticated`), and the handler calls `let ctx = caller.authenticate(&ports, &state.clock).await?;` right after `state.ports()` (a bad token is 401 `invalid_token`), so one request uses one pooled connection. `/me` routes take `StaffCaller` the same way (API keys get 401 `session_required`). Work that must outlive the response (`Storage::spawn_password_reset`) is spawned per concrete storage, for the same `Send` reason as `router!`. `HostEnv` (`state.rs`) provides OS randomness, Argon2id on blocking threads, the `SecretBox` under `HYPHA_SECRET_KEY`, and a logging `Notifier`.

Command shape (see `crates/hypha-products/src/commands.rs`): `async fn cmd(ctx: &Ctx, repo: &impl Repo, ids: &impl IdGenerator, input: RawInput) -> Result<_, CommandError>`; first line `ctx.require(Scope::...)?`, then validate raw input through domain constructors. `CommandError` composes lower errors via `#[from]`.

When a command needs more than one repository, take `ports: &S` bound on the accessor traits it uses: each crate defines `HasXxx` (associated repo type + `fn xxx(&self)`) next to its repo traits in `repo.rs`, e.g. `create_cart<S: HasMarkets + HasCarts>`. A composing crate names its union with a blanket impl (`CartPorts`, `PricingPorts` in `crates/hypha-cart/src/commands.rs`), so implementors never list them. Hosts and tests implement the accessors once (`hypha_testkit::InMemoryPorts`, `PgStore`); a new repository adds one `HasXxx`. Static dispatch keeps `Send` provable; don't bundle them into `&dyn` objects.

## Hard constraints

- Core and domain crates must compile for `wasm32-unknown-unknown`: no Tokio, threads, `std::fs`, `SystemTime::now()` or random UUID generation there. Time, IDs and randomness come in through ports.
- Repository traits use `async fn` with `#[allow(async_fn_in_trait)]` and **no `Send` bound**, on purpose (Cloudflare futures are `!Send`). Hosts use concrete types/generics so the compiler can prove `Send`; never `&dyn` for ports used across `.await`.
- Authorization lives in commands (`ctx.require(Scope::...)?`); authentication lives in hosts. Commands never trust raw input.

## Code standards

- Money: integer minor units (`i64`), never floats, capped at ±`Money::MAX_AMOUNT` (2⁵³ − 1) so JSON clients represent every amount exactly; `Money::new` is fallible. Checked arithmetic returning `Result`; percentages as `BasisPoints` with round-half-away-from-zero; `i128` for intermediates.
- Newtypes with validated constructors for anything with rules (`Handle`, `Price`, `CountryCode`, IDs). Don't derive `Deserialize` on validated types unless via `#[serde(try_from = ...)]`.
- IDs: `define_id!` macro in `hypha-core/src/ids.rs` — UUID inside, prefixed string at the edges (`prod_…`, `var_…`, `mkt_…`, `pl_…`). Store native UUIDs; UUIDv7 in hosts.
- Errors: `thiserror` enums with specific variants (no stringly errors). Every error enum implements `hypha_core::ErrorCode` with an exhaustive `match` (no `_` arm): stable snake_case codes, never renamed once released, wrappers delegate to the inner error. Messages use `Display` for domain values (`EUR`, `active`); raw input the caller sent (a string or char that failed to parse) is quoted with `{0:?}`, so blanks and control characters stay visible. No `unwrap`/`expect`/panics in non-test code except documented invariants.
- Aggregates own their children (Product owns Variants); cross-aggregate rules live in commands/functions above them.
- Migrations are per module: `hypha_store_postgres::CORE` is core's `MigrationSet`; a spore ships its own set (module `"spore_<name>"`, tables prefixed the same way), passed to `migrate(&mut client, &[CORE, …])`. Append only: an applied migration is never edited, renamed or reordered (the runner checks names and fails).
- Persistence (design note `docs/design/0001-postgres-storage.md`): aggregate roots implement `Versioned`; repos have `insert(&mut T)` / `update(&mut T)`, never an upsert. `update` from an old version fails with `RepoError::StaleVersion`. Uniqueness rules are repo `keys::*` constants reported as `RepoError::Duplicate(key)`; commands keep a read-then-check for precise errors **and** map the `Duplicate` from the write to the same domain error (a race must look like a normal duplicate). In-memory repos are built on `hypha-testkit`'s `Table`, which enforces versions and keys like a database. A command writes one aggregate through its repo, or several through one `HasBatch` commit (`hypha-core/src/ports.rs`): each crate has a `StageXxx` trait next to its repo trait (`StageCarts`), implemented by the adapters' batch types (`InMemoryBatch`, `PgBatch`), and staging moves the aggregate to the version it gets on commit. `reserve_stock` / `release_stock` (no batch of their own) still write every item at once through `InventoryItemRepo::update_many`, all or nothing (0004, I7). `restore` re-runs constructor rules, so tightening a rule (a shorter max length, a newly required field) needs a migration that fixes or rejects existing rows first, or those rows stop loading (500).
- Getters return values for `Copy` types and references (`&str`, slices) for owned heap data.
- Workspace lints: `unsafe_code = "forbid"`, `clippy::all = "warn"`. Code must be fmt-clean and clippy-warning-free.

## Testing

- Unit tests in `#[cfg(test)] mod tests` at the bottom of each file; one rule per test; test boundaries (0 vs -1, exact rounding halves); compare expected errors exactly with `assert_eq!(result, Err(...))`, never via `unwrap`.
- Command tests are integration tests in each crate's `tests/` folder using `hypha-testkit` (avoids duplicate-crate issues with dev-dependencies).
- Async in tests via `pollster::block_on`; deterministic IDs via `SequentialIds` and `Uuid::from_u128(n)`; time via `FixedClock` (commands take `clock: &impl Clock` like `ids`).
- Repository contract: `hypha_testkit::contract::{product_repo, market_repo, catalog_repo, price_list_repo, cart_repo, tax_class_repo, tax_region_repo, location_repo, inventory_item_repo, customer_repo, order_repo, idempotency_repo, batch, identity_repo, session_repo, api_key_repo, password_reset_repo, user_repo, role_repo, invite_repo, staff_batch}` define what every repo adapter must do (round trips, versions, unique keys, batched methods). The suite is behind testkit's `contract` feature (so hosts don't ship it): adapters enable it as a dev-dependency, and the in-memory run (`crates/hypha-testkit/tests/contract.rs`) needs `--features contract` when testing that crate alone (`cargo test --workspace` enables it). Every new adapter must run it. Batch methods state caller preconditions (parent exists, no duplicates) in their docs, and the contract checks that breaking them fails. When a repo trait gains a method or a unique key, add a contract scenario first.

## Conventions

- Conventional Commits (`feat(products): …`, `refactor(core): …`); pure moves/refactors in separate commits from features.
- LF line endings (enforced by `.gitattributes`). Windows dev machine; prefer cross-platform commands.

## Working with Danilo

- He's learning Rust by building this. Default to explaining *why* and naming the Rust concept involved (ownership, traits, lifetimes, etc.), and review his code rather than rewriting whole files unprompted. When he explicitly asks for code, write it fully.
- Flag hard-to-reverse design decisions (public API shapes, `Send` bounds, storage schema) instead of choosing silently.

## Roadmap / deferred (don't build unless asked)

Tenancy (`docs/design/0006-multi-tenancy.md`): one store per deployment (model A); a store is resolved by the host and passed to `Storage::ports(store)`. Domain crates, aggregates and commands never see a store ID.

Extensibility direction (custom fields, policies, events/outbox/webhooks, spores, TS-first external apps): `docs/design/0002-extensibility.md`. Its section 7 lists what to reserve while building core features.

Order follow-ups from `docs/design/0007-orders.md` §10 (addresses and shipping, fulfillment, cancellation and refunds, payment spores, listing orders, the outbox), auth follow-ups from `docs/design/0008-auth.md` §9 (login rate limiting, passkeys, SSO/OIDC, customer accounts, OAuth apps, step-up, listing users and keys, an email provider), combined listings (variants as products), FX rates as a port, MCP tool generation from commands, UCP adapter for agentic shopping (via ucphub.ai), admin AI features as plugins behind a `ModelProvider` port.
