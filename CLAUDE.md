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
cargo build -p hypha-core -p hypha-products -p hypha-markets -p hypha-pricing -p hypha-cart -p hypha-store-postgres --target wasm32-unknown-unknown
```

Extend the wasm build list when a domain crate or storage adapter is added.

- Single crate's tests: `cargo test -p hypha-products`
- Single test by name: `cargo test -p hypha-products rejects_same_handle_products`
- Only an integration test file: `cargo test -p hypha-products --test commands`
- Run the HTTP host (binds `127.0.0.1:3000`, `RUST_LOG` respected): `cargo run -p hypha-host-axum` (in memory), or with `DATABASE_URL=postgres://hypha:hypha@localhost:5432/hypha` for Postgres (migrations run on startup).
- End-to-end check against a freshly started host (Git Bash on Windows): `bash scripts/e2e.sh`. Asserts status + error `code` per step, exits non-zero on failure. Run it after changing routes, DTOs or error mapping, and add a step for new routes. Against Postgres, use a fresh database (the script reuses handles and countries): `docker compose exec -T postgres psql -U hypha -c "DROP DATABASE IF EXISTS hypha_e2e" -c "CREATE DATABASE hypha_e2e"`, then start the host with `DATABASE_URL=.../hypha_e2e`.
- Postgres (Docker Desktop): `docker compose up -d`, then `DATABASE_URL=postgres://hypha:hypha@localhost:5432/hypha cargo test -p hypha-store-postgres` runs the repository contract against it (each test gets its own schema). Without `DATABASE_URL` those tests skip themselves.

## Architecture (hexagonal / ports and adapters)

Dependencies point inward: hosts → domain crates → core. Core and domain crates never depend on storage, HTTP or runtimes.

Domain crates are layered; a crate may only depend on layers below it (Cargo rejects cycles, so a wrong edge forces a painful restructure later):

| Layer | Crates | May depend on |
| --- | --- | --- |
| Hosts | `hypha-host-axum` | anything |
| Storage adapters | `hypha-store-postgres` (tokio-postgres; native and Workers via Hyperdrive) | domain crates, core; never hosts |
| Composing domains | `hypha-cart` (later orders) | leaf domains, core |
| Leaf domains | `hypha-products`, `hypha-markets`, `hypha-pricing` | core; `hypha-pricing` → `hypha-markets` only |
| Core | `hypha-core` | nothing internal |

Leaf functions take the plain values they need (IDs, `Price`), not another leaf's aggregate. E.g. `resolve_price` takes `ProductId`, `VariantId` and the base `Price` rather than `&Product`; the composing command (cart) loads the product and passes them in. Ask before adding a new edge between domain crates.

- `hypha-core`: shared types only — `Money`/`Price`/`BasisPoints` (`money/`), prefixed IDs (`ids.rs`), `CountryCode`, `Scope`, `Ctx`/`Actor`, generic ports (`IdGenerator`, `RepoError` in `ports.rs`). No domain logic, no I/O.
- Domain crates (`hypha-products`, `hypha-markets`, `hypha-pricing`, `hypha-cart`): aggregates with private fields and validated constructors, a repository trait (port) per aggregate in `repo.rs`, error enums in `error.rs`, and commands in `commands.rs` (`hypha-products` is the simplest template; `hypha-cart` shows a command that composes several domains).
- `hypha-testkit`: in-memory repos for every port (`InMemoryProductRepo`, `InMemoryCartRepo`, …) and deterministic ports (`SequentialIds`). It depends on domain crates, so domain crates use it only as a dev-dependency from their `tests/` folder.
- `hypha-host-axum`: the only place with Tokio, HTTP, auth parsing, logging subscribers (`tracing`) and real ID generation (`UuidV7Ids`). `routes/` holds one module per area with its own request/response DTOs (never serialize domain types directly); `error.rs` maps every crate's `CommandError` to a status (403 scope, 404 resource in the URL, 409 conflict, 422 invalid input incl. unknown IDs in the body, 500 storage with the message hidden); `extract.rs` wraps `Json`/`Path` so even axum's rejections return `{"code": …, "error": …}`. Storage is chosen at startup (`state.rs`): `MemoryStorage` (testkit repos) or `PgStorage` (deadpool pool; one `PgStore` per request) behind the `Storage` trait; handlers are generic over it and call `state.storage.ports()` once per request. Routes are listed once in the `router!` macro in `routes/mod.rs`, stamped out per concrete storage, because axum can only prove handler futures `Send` for concrete types (add new routes there). Currently uses `Ctx::system()` (no auth yet).

Command shape (see `crates/hypha-products/src/commands.rs`): `async fn cmd(ctx: &Ctx, repo: &impl Repo, ids: &impl IdGenerator, input: RawInput) -> Result<_, CommandError>`; first line `ctx.require(Scope::...)?`, then validate raw input through domain constructors. `CommandError` composes lower errors via `#[from]`.

When a command needs more than two or three ports, group them in a `XxxPorts` trait with associated types (see `CartPorts` in `crates/hypha-cart/src/commands.rs`) and take `ports: &impl XxxPorts`. Hosts and test worlds implement it once; static dispatch keeps `Send` provable. Don't bundle them into `&dyn` objects.

## Hard constraints

- Core and domain crates must compile for `wasm32-unknown-unknown`: no Tokio, threads, `std::fs`, `SystemTime::now()` or random UUID generation there. Time, IDs and randomness come in through ports.
- Repository traits use `async fn` with `#[allow(async_fn_in_trait)]` and **no `Send` bound**, on purpose (Cloudflare futures are `!Send`). Hosts use concrete types/generics so the compiler can prove `Send`; never `&dyn` for ports used across `.await`.
- Authorization lives in commands (`ctx.require(Scope::...)?`); authentication lives in hosts. Commands never trust raw input.

## Code standards

- Money: integer minor units (`i64`), never floats, capped at ±`Money::MAX_AMOUNT` (2⁵³ − 1) so JSON clients and D1 represent every amount exactly; `Money::new` is fallible. Checked arithmetic returning `Result`; percentages as `BasisPoints` with round-half-away-from-zero; `i128` for intermediates.
- Newtypes with validated constructors for anything with rules (`Handle`, `Price`, `CountryCode`, IDs). Don't derive `Deserialize` on validated types unless via `#[serde(try_from = ...)]`.
- IDs: `define_id!` macro in `hypha-core/src/ids.rs` — UUID inside, prefixed string at the edges (`prod_…`, `var_…`, `mkt_…`, `pl_…`). Store native UUIDs; UUIDv7 in hosts.
- Errors: `thiserror` enums with specific variants (no stringly errors). Every error enum implements `hypha_core::ErrorCode` with an exhaustive `match` (no `_` arm): stable snake_case codes, never renamed once released, wrappers delegate to the inner error. Messages use `Display` (`EUR`, `active`), not `{:?}`. No `unwrap`/`expect`/panics in non-test code except documented invariants.
- Aggregates own their children (Product owns Variants); cross-aggregate rules live in commands/functions above them.
- Persistence (design note `docs/design/0001-postgres-storage.md`): aggregate roots implement `Versioned`; repos have `insert(&mut T)` / `update(&mut T)`, never an upsert. `update` from an old version fails with `RepoError::StaleVersion`. Uniqueness rules are repo `keys::*` constants reported as `RepoError::Duplicate(key)`; commands keep a read-then-check for precise errors **and** map the `Duplicate` from the write to the same domain error (a race must look like a normal duplicate). In-memory repos are built on `hypha-testkit`'s `Table`, which enforces versions and keys like a database. A command writes at most one aggregate.
- Getters return values for `Copy` types and references (`&str`, slices) for owned heap data.
- Workspace lints: `unsafe_code = "forbid"`, `clippy::all = "warn"`. Code must be fmt-clean and clippy-warning-free.

## Testing

- Unit tests in `#[cfg(test)] mod tests` at the bottom of each file; one rule per test; test boundaries (0 vs -1, exact rounding halves); compare expected errors exactly with `assert_eq!(result, Err(...))`, never via `unwrap`.
- Command tests are integration tests in each crate's `tests/` folder using `hypha-testkit` (avoids duplicate-crate issues with dev-dependencies).
- Async in tests via `pollster::block_on`; deterministic IDs via `SequentialIds` and `Uuid::from_u128(n)`.
- Repository contract: `hypha_testkit::contract::{product_repo, market_repo, catalog_repo, price_list_repo, cart_repo}` define what every repo adapter must do (round trips, versions, unique keys, batched methods). The in-memory repos run them in `crates/hypha-testkit/tests/contract.rs`, and every new adapter must run them too. When a repo trait gains a method or a unique key, add a contract scenario first.

## Conventions

- Conventional Commits (`feat(products): …`, `refactor(core): …`); pure moves/refactors in separate commits from features.
- LF line endings (enforced by `.gitattributes`). Windows dev machine; prefer cross-platform commands.

## Working with Danilo

- He's learning Rust by building this. Default to explaining *why* and naming the Rust concept involved (ownership, traits, lifetimes, etc.), and review his code rather than rewriting whole files unprompted. When he explicitly asks for code, write it fully.
- Flag hard-to-reverse design decisions (public API shapes, `Send` bounds, storage schema) instead of choosing silently.

## Roadmap / deferred (don't build unless asked)

Cart quantity/remove commands, orders, Postgres adapter (sqlx), API keys then OAuth, structured logging (tracing), combined listings (variants as products), FX rates as a port, MCP tool generation from commands, UCP adapter for agentic shopping (via ucphub.ai), admin AI features as plugins behind a `ModelProvider` port.
