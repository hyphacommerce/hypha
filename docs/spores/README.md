# Writing a spore

A spore is a Rust crate compiled into a Hypha deployment's API (design note 0030): a
payment provider, a carrier, an integration that's simpler inside the API than as a
service of its own. It implements the same contracts external apps do (0029), so core
can't tell them apart, and it's trusted code: the operator chose it by building it in.

Use an [app](../apps/) instead for anything a third party runs as a service, anything in
another language, or anything a store should install without rebuilding its API.

The Stripe spore ([hyphacommerce/spore-stripe](https://github.com/hyphacommerce/spore-stripe)) is
the worked example, built outside this repository as yours will be: read it beside this page.
The wishlist spore ([hyphacommerce/spore-wishlist](https://github.com/hyphacommerce/spore-wishlist))
is the tutorial: a table of its own, storefront routes for signed-in shoppers, an operator
setting and an admin block, walked through on the docs site (`guides/spore-tutorial`).

## The crate

```toml
[package]
name = "acme-hypha-reviews"
edition = "2024"

# Not part of a host's workspace.
[workspace]

[dependencies]
# Everything a spore may use, at one version. Not Hypha's internal crates: those change
# without notice. From git until Hypha's crates are published; Cargo.lock pins the commit.
hypha-spore = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["http"] }  # "postgres" for your own tables

[dev-dependencies]
hypha-spore = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["http", "testing"] }
# Route tests against a real host (see Testing).
hypha-host-axum = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["testing"] }
```

While Hypha's repository is private, let Cargo fetch it with your git credentials: in the
spore's `.cargo/config.toml`, `[net] git-fetch-with-cli = true`.

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
  own rules (`"x-hypha-order": [...]` on the schema gives the fields' order); the admin
  draws the form, and the spore reads them with
  `env.settings::<T>(&ports)` (secrets opened). A payment provider gets them in
  `PaymentRequest::store_settings` instead (it has no ports).
- **Custom fields** (design note 0033, §8): `custom_fields()` returns a
  `hypha_spore::custom_fields::DefinitionSpec` per field. Each is in the namespace
  `spore_<name>`, so its `key` is enough: `points` is addressed as
  `spore_loyalty.points`. The host syncs them at every start, right after it registers
  the spore's app. New keys are created and changed ones updated. A key the spore no
  longer declares is **deleted with its values**. A changed `field_type` stops the API,
  naming the spore and the field: declare a new key instead. A Worker syncs nothing, so
  it refuses to serve until `hypha-host-axum setup` (built with the same spores) has
  synced them.
  - The spore writes its values as itself (`Actor::App`, with its `SCOPES`):
    `env.set_custom_fields(&ports, &Owner::product(id), values)`, where `values` is a
    JSON Merge Patch by full key (`{"spore_loyalty.points": 120}`, `null` removes).
    `env.custom_fields(&ports, &owner)` reads them, its private ones included.
  - Like any write, these need the owner's scope among `SCOPES` (`products:write` to
    write a product's fields, `products:read` to read them). The owner must exist: the
    caller loaded it.
  - Only the spore writes its fields, plus staff where the field's `access.admin` is
    `read_write`.

```rust
use hypha_spore::custom_fields::{Access, Capabilities, DefinitionSpec, FieldType, OwnerType};

fn custom_fields() -> Vec<DefinitionSpec> {
    vec![DefinitionSpec {
        owner_type: OwnerType::Customer,
        key: "points".to_string(),
        field_type: FieldType::parse("integer").expect("a type"),
        name: "Loyalty points".to_string(),
        description: None,
        validations: Default::default(),
        access: Access::default(),
        private: false,
        personal: false,
        capabilities: Capabilities { filterable: true, unique: false },
    }]
}
```

## What it can add

| | How |
| --- | --- |
| A payment provider | `PaymentProvider` in `PaymentProviders`, keyed `spore_<name>` (or `spore_<name>_…`); `ActionAnswer::Completed` when the provider's answer says it's done |
| Event handlers | `EventHandler<P>` in `Handlers`, bounded on the ports it reads; named `spore_<name>_…` |
| Custom fields | `Spore::custom_fields()`, synced at startup in `spore_<name>`; values written through `env.set_custom_fields` |
| Routes | `hypha_spore::http::spore_routes!` at the crate root: paths under `/spores/<name>/` (public, rate limited), `/admin/spores/<name>/`, `/storefront/spores/<name>/`; handlers generic over `S: Storage`, annotated with `#[utoipa::path]` |
| Admin blocks and actions | `SporeUi<P>`: `blocks()`, `actions()` (an action may stand for a payment point), `block`, `action`, on the targets apps use |
| Storefront elements | `SporeUi::elements()`: a web component's tag and the path of the spore's public route serving its module (with `Access-Control-Allow-Origin: *`), listed at the API's public address: set `server.public_url` (`hypha deploy` does) when storefronts reach the API at an internal one. Bundle a TypeScript element into the `.js` the route serves (`include_str!`); the Stripe spore's `scripts/build-element.mjs` does it with Vite |

A disabled spore (staff's switch in the admin) does nothing: its providers are
unavailable, its handlers' deliveries wait on the retry schedule, its routes answer
`403 app_disabled`, its UI isn't shown.

## Testing

- Unit and command tests on in-memory ports: `hypha_spore::testing` (`InMemoryPorts`,
  `TestEnv`, `env("<name>")`, and `env_with_http("<name>", fake)` to answer its HTTP
  with a fake of the service it calls).
- A host built with it, on Postgres: `hypha_host_axum::testing` (feature `testing`).
  Declare the deployment, then make a host per test and send it requests:

  ```rust
  use hypha_host_axum::testing::{app_with, send};

  hypha_host_axum::deployment! { enum WithReviews {
      spores: [acme_hypha_reviews::Reviews],
      routes: [acme_hypha_reviews::reviews_routes],
  } }

  #[tokio::test]
  async fn reviews_are_listed() {
      let app = app_with::<WithReviews>("[spores.reviews]\nlimit = 5").await;
      let (status, json) = send(&app, "GET", "/admin/spores/reviews/list", None).await;
      assert_eq!(status, 200, "{json}");
  }
  ```

  Each test gets a database of its own on the Postgres at `HYPHA_TEST_DATABASE_URL`
  (Hypha's compose `postgres-test`: `docker compose up -d` in a Hypha checkout). `send`
  uses a secret key with every scope; `send_as` and `send_with` take other credentials;
  `publishable`, `market_selling_a_mug` and `cart_for_three_mugs` set up a shop. The
  Stripe spore's `tests/routes.rs` drives a whole checkout against a fake Stripe.

## Building it into a deployment

- A first-party spore by name: `hypha spores add stripe` (or `hypha init --spores
  stripe`), pinned to its repository's head by the next deploy.
- Any other: `hypha spores add reviews --crate acme-hypha-reviews --type
  acme_hypha_reviews::Reviews --routes acme_hypha_reviews::reviews_routes --git <url>
  [--rev <commit>]` (or `--version`, or `--path` to a folder under this checkout's
  `spores/`). `hypha deploy` clones git spores at their commit with this machine's git
  credentials, writes the deployment's own host crate (`hypha_host_axum::host! {
  spores: [...], routes: [...] }`, with Hypha's git source patched to this checkout so
  the spores and the host share its crates) and builds the API from it; on Cloudflare
  also its own Worker crate (`hypha_host_workers::worker! { … }`), all under
  `.hypha-build/<deployment>/`.
- Locally: `HYPHA_SPORES=stripe pnpm dev` builds the API with first-party spores, each
  from its checkout beside this one (`../spore-stripe`) when there is one.
- Its app (`spore_<name>`) is registered when the API starts: enabled, its scopes as its
  grant. Taken out of the build, it's listed as missing; its settings and tables stay.
