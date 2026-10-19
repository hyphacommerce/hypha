# Writing a spore

A spore is a Rust crate compiled into a Hypha deployment's API (design note 0030): a
payment provider, a carrier, an integration that's simpler inside the API than as a
service of its own. It implements the same contracts external apps do (0029), so core
can't tell them apart, and it's trusted code: the operator chose it by building it in.

Use an [app](../apps/) instead for anything a third party runs as a service, anything in
another language, or anything a store should install without rebuilding its API.

The Stripe spore (`spores/stripe`) is the worked example: read it beside this page.

## The crate

```toml
[package]
name = "acme-hypha-reviews"
edition = "2024"

[dependencies]
# Everything a spore may use, at one version (design note 0030, SP6). Not Hypha's
# internal crates: those change without notice.
hypha-spore = { version = "0.1", features = ["http"] }  # "postgres" for your own tables

[dev-dependencies]
hypha-spore = { version = "0.1", features = ["http", "testing"] }
```

It must build for `wasm32-unknown-unknown` (Hypha runs on Cloudflare Workers too): no
Tokio, threads, `std::fs` or system time. Time, IDs, randomness, HTTP and secrets come
from its environment (`SporeEnv`).

## The `Spore` trait

```rust
use hypha_spore::core::Scope;
use hypha_spore::migrations::MigrationSet;
use hypha_spore::{Spore, SporeEnv, SporeError};

pub struct Reviews {
    pub env: SporeEnv,
}

impl Spore for Reviews {
    /// Its key is `spore_reviews`, its tables `spore_reviews_*`. Never rename it.
    const NAME: &'static str = "reviews";
    /// What it acts with when it calls commands (as `Actor::App`), shown to staff.
    const SCOPES: &'static [Scope] = &[Scope::ProductsRead];

    fn title() -> &'static str {
        "Reviews"
    }

    fn migrations() -> MigrationSet {
        MigrationSet { module: "spore_reviews", migrations: &[/* … */] }
    }

    /// Built once at startup; a failure stops the API, naming the spore.
    fn new(env: SporeEnv) -> Result<Self, SporeError> {
        Ok(Reviews { env })
    }

    type PaymentProviders = ();
    type Handlers = ();
    fn payment_providers(&self) {}
    fn handlers(&self) {}
}

/// No admin UI: an empty impl.
impl<P> hypha_spore::SporeUi<P> for Reviews {}
```

- **Its own tables only** (§5): its migrations may create objects named
  `spore_<name>_*`, foreign keys into core's tables that `CASCADE` or `SET NULL` on
  delete, and nothing on core's tables. The API checks the catalog after each migration
  and refuses one that breaks this. Core data changes through core's commands.
- **Following core** (§6): an `EventHandler` in `Handlers` hears core's events, at least
  once (make it idempotent). That's how a spore reacts to an order placed or cancelled.
- **Operator configuration**: its `[spores.<name>]` section of `hypha.config.toml`,
  read with `env.config_as::<T>()` (an API base for tests, a timeout). Never secrets.
- **Merchant settings** (§8): `settings_schema()` returns a small JSON Schema (scalar
  fields; `"x-hypha-secret": true` for sealed, write-only ones) and `check_settings` its
  own rules; the admin draws the form, and the spore reads them with
  `env.settings::<T>(&ports)` (secrets opened). A payment provider gets them in
  `PaymentRequest::store_settings` instead (it has no ports).

## What it can add

| | How |
| --- | --- |
| A payment provider | `PaymentProvider` in `PaymentProviders`, keyed `spore_<name>` (or `spore_<name>_…`); `ActionAnswer::Completed` when the provider's answer says it's done |
| Event handlers | `EventHandler<P>` in `Handlers`, bounded on the ports it reads; named `spore_<name>_…` |
| Routes | `hypha_spore::http::spore_routes!` at the crate root: paths under `/spores/<name>/` (public, rate limited), `/admin/spores/<name>/`, `/storefront/spores/<name>/`; handlers generic over `S: Storage`, annotated with `#[utoipa::path]` |
| Admin blocks and actions | `SporeUi<P>`: `blocks()`, `actions()` (an action may stand for a payment point), `block`, `action`, on the targets apps use |
| Storefront elements | `SporeUi::elements()`: a web component's tag and the path of the spore's public route serving its module (with `Access-Control-Allow-Origin: *`) |

A disabled spore (staff's switch in the admin) does nothing: its providers are
unavailable, its handlers' deliveries wait on the retry schedule, its routes answer
`403 app_disabled`, its UI isn't shown.

## Testing

- Unit and command tests on in-memory ports: `hypha_spore::testing` (`InMemoryPorts`,
  `TestEnv`, `env("<name>")`, and `env_with_http("<name>", fake)` to answer its HTTP
  with a fake of the service it calls).
- A host built with it: in this repository, the route tests' `app_with_spores::<D>()`
  with `crate::deployment! { enum D { spores: [X], routes: [x_routes] } }`
  (`crates/hypha-host-axum/src/route_tests/stripe.rs`).

## Building it into a deployment

- This repository's spores are features of the hosts: `cargo run -p hypha-host-axum
  --features spore-stripe`, `HYPHA_SPORES=stripe pnpm dev`, and in a deployment
  `hypha spores add stripe` (or `hypha init --spores stripe`).
- Any other: `hypha spores add reviews --crate acme-hypha-reviews --type
  acme_hypha_reviews::Reviews --routes acme_hypha_reviews::reviews_routes --git <url>
  --rev <commit>` (or `--version`, `--path`). `hypha deploy` writes the deployment's own
  host crate (`hypha_host_axum::host! { spores: [...], routes: [...] }`) and builds the
  API from it. Not on Cloudflare yet.
- Its app (`spore_<name>`) is registered when the API starts: enabled, its scopes as its
  grant. Taken out of the build, it's listed as missing; its settings and tables stay.
