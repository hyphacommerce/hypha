---
title: Write a spore
description: Rust crates compiled into the API, on the same contracts as apps.
sidebar:
  order: 3
---

A **spore** is a Rust crate compiled into a deployment's API: a payment provider, a
carrier, an integration that's simpler inside the API than as a service of its own. It
implements the same contracts apps do, so the core can't tell them apart, and it's trusted
code: the operator chose it by building it in.

Use an [app](/docs/guides/apps/) instead for anything a third party runs as a service,
anything in another language, or anything a store should install without rebuilding its
API.

The Stripe spore, [spore-stripe](https://github.com/hyphacommerce/spore-stripe), is the
worked example, built outside Hypha's repository as yours will be.

## The crate

```toml
[package]
name = "acme-hypha-reviews"
edition = "2024"

# Not part of a host's workspace.
[workspace]

[dependencies]
hypha-spore = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["http"] }

[dev-dependencies]
hypha-spore = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["http", "testing"] }
hypha-host-axum = { git = "https://github.com/hyphacommerce/hypha", branch = "main", features = ["testing"] }
```

`hypha-spore` is everything a spore may use, at one version; Hypha's internal crates
change without notice. It comes from git until Hypha's crates are published, with
`Cargo.lock` pinning the commit.

A spore must build for `wasm32-unknown-unknown`, since Hypha runs on Cloudflare Workers
too: no Tokio, threads, files or system time. Time, IDs, randomness, HTTP and secrets come
from its environment.

## What it can add

| | How |
| --- | --- |
| A payment provider | A `PaymentProvider`, keyed `spore_<name>` |
| Event handlers | An `EventHandler` for the event types it wants, run by the dispatcher |
| Custom fields | Declared by the spore, in its own namespace |
| Routes | Under `/spores/<name>/` (public, rate limited), `/admin/spores/<name>/` and `/storefront/spores/<name>/` |
| Admin blocks and actions | On the same targets apps use |
| Storefront elements | A web component served from one of its routes |
| Its own tables | A migration set of its own, in tables prefixed `spore_<name>_` |

A spore has settings staff edit in the admin (secret fields sealed), and a configuration
section in `hypha.config.toml` for the operator. Staff can switch a spore off: its
providers become unavailable, its routes answer `403 app_disabled`, and its event
deliveries wait.

## Testing

- **Commands and handlers** on in-memory ports: `hypha_spore::testing` gives an
  environment with a fixed clock, sequential IDs and a fake for the HTTP service you call.
- **A host built with it**, on Postgres: `hypha_host_axum::testing`. Declare the
  deployment, then make a host per test and send it requests:

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

Each test gets a database of its own on the Postgres at `HYPHA_TEST_DATABASE_URL`. The
test kit also sets up a shop: a publishable key, a market selling a product, a cart ready
to pay.

## Building it into a deployment

```bash
pnpm hypha spores add reviews --crate acme-hypha-reviews \
  --type acme_hypha_reviews::Reviews --routes acme_hypha_reviews::reviews_routes \
  --git https://github.com/acme/hypha-reviews
pnpm hypha deploy
```

The deploy pins the spore to its repository's head (or the `--rev` you give), clones it,
and builds the API with it. First-party spores go by name: `pnpm hypha spores add stripe`.
When the API starts, the spore's app is registered, enabled, with its scopes as its grant.
