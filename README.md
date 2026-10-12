# Hypha

> Working name. Early-stage and not ready for production use.

Hypha is an open-source, modular e-commerce API written in Rust. The same core is meant to run anywhere, from Cloudflare Workers (WASM) and AWS Lambda to Vercel and a plain VPS. It is built to be easy for AI coding agents to extend, and for AI assistants to operate from the admin panel.

## Goals

- **Modular.** Each domain (products, markets, pricing, cart, orders…) is its own crate. You extend behavior with plugins ("spores") built on traits and events, so you never have to fork the core.
- **Agent-friendly.** The codebase is meant to be easy for coding agents to extend: small crates, explicit types, exact errors, fast deterministic tests. Every operation is a *command* with typed input, required scopes and typed errors, so the same surface can be exposed over HTTP, MCP and a CLI for admin assistants (`Actor::Agent`, with their own scopes). Agentic shopping is handled separately by a planned [Universal Commerce Protocol](https://ucp.dev) adapter.
- **Built for large catalogs.** Variants are sparse and explicit, never generated as a Cartesian product of options. Markets, catalogs and price lists follow the Shopify model.
- **Runs on many hosts.** Core and domain crates compile to `wasm32-unknown-unknown` and contain no runtime, storage or HTTP code. Each deployment target is a thin host crate.

## Status

| Area | State |
| --- | --- |
| Core types: money, prices, basis points, prefixed IDs, countries, scopes, actor context | ✅ |
| Products: handles, options, explicit variants, status transitions, create/get commands | ✅ |
| Markets, catalogs (one per market) and price lists, with `assign_price_list` | ✅ |
| HTTP host (axum) with in-memory storage: products, markets, price lists, catalogs, carts; JSON errors everywhere | ✅ |
| Price resolution: fixed price, else base price adjusted by basis points | ✅ |
| Cart: create, add line by variant, get at current prices with per-line availability | ✅ |
| Orders | ⏳ planned |
| Postgres adapter, API keys / OAuth, MCP tools, Cloudflare / Lambda hosts | ⏳ planned |

## Architecture

Hypha uses a hexagonal (ports and adapters) architecture. Dependencies only point inward:

```
hosts (axum, later Cloudflare / Lambda)  →  domain crates  →  hypha-core
```

| Crate | Role |
| --- | --- |
| `hypha-core` | Shared types only: `Money`/`Price`/`BasisPoints`, prefixed IDs, `CountryCode`, `Scope`, `Ctx`/`Actor`, and generic ports (`IdGenerator`, `RepoError`). Contains no domain logic and no I/O. |
| `hypha-products`, `hypha-markets`, `hypha-pricing`, `hypha-cart` | Domain crates. Each holds aggregates with validated constructors, repository traits (ports) and commands. |
| `hypha-testkit` | In-memory repositories and deterministic ports (`SequentialIds`) for tests. |
| `hypha-host-axum` | HTTP host. This is the only crate that uses Tokio, HTTP, logging or real (UUIDv7) ID generation. |

Some key design rules:

- **No I/O in the core.** Time, IDs and randomness reach the core only through ports. As a result, the core builds for WASM, and tests are deterministic.
- **Repository traits have no `Send` bound**, because futures on Cloudflare Workers are `!Send`. Hosts work with concrete types, which lets the compiler prove `Send` wherever it is needed.
- **Authorization lives in commands** (`ctx.require(Scope::ProductsWrite)?`). Authentication lives in hosts.
- **Money is stored as integer minor units** (`i64`) and uses checked arithmetic. Percentages are basis points and round half away from zero. Floats are never used.
- **IDs are UUIDs internally.** At the API boundary they become prefixed strings: `prod_…`, `var_…`, `mkt_…`, `pl_…`.

## Getting started

Requires a recent stable Rust toolchain (the code uses edition 2024) and the WASM target:

```bash
rustup target add wasm32-unknown-unknown
```

Run the tests:

```bash
cargo test --workspace
```

Check that the core still builds for WASM:

```bash
cargo build -p hypha-core -p hypha-products -p hypha-markets -p hypha-pricing -p hypha-cart --target wasm32-unknown-unknown
```

Start the HTTP host on `http://127.0.0.1:3000` (data lives in memory). Set the log level with `RUST_LOG`:

```bash
cargo run -p hypha-host-axum
```

Create a product. Amounts are in minor units, so `1000` = €10.00:

```bash
curl -X POST http://127.0.0.1:3000/products -H "content-type: application/json" -d '{"handle":"mug","title":"Mug","price":{"amount":1000,"currency":"EUR"}}'
```

Then fetch it using the `prod_…` ID from the response:

```bash
curl http://127.0.0.1:3000/products/<id>
```

## Contributing

Before you open a PR, make sure all of these pass:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

Also run the WASM build shown above. Commits follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat(products): …`, `refactor(core): …`). Keep pure refactors in separate commits from features.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
