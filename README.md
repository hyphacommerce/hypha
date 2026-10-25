# Hypha

**An open-source commerce API in Rust, with an admin panel and a storefront.** It runs on
a plain server or on Cloudflare Workers, grows through apps and Rust plugins instead of
forks, and lets AI agents operate a store safely: every change an agent makes can be
previewed exactly and approved by a person first.

> **Public beta, under active development.** Hypha works end to end (catalog, checkout,
> payments, fulfilment, accounts, apps), but it isn't production-proven yet, and its APIs
> may still change between 0.x releases.

## What's in it

| Area | What you get |
| --- | --- |
| Catalog | Products with options and sparse, explicit variants (never the Cartesian product), categories, media, rich text, custom fields, search with facets |
| Selling | Markets (countries and currency), catalogs, sales channels, tax regions and classes |
| Prices | Price lists that compete on the lowest price: sales and clearances on a schedule, compare-at prices, quantity breaks, prices for customer groups, a spreadsheet editor with CSV import |
| Discounts | Codes and automatic discounts: amounts or percentages off products, the order or shipping, buy X get Y, volume tiers, combinations, usage limits |
| Checkout | Carts priced per market, shipping zones and rates (or a rate app), payment methods: manual (bank transfer, cash on delivery) and online payments started before the order exists |
| Orders | Fulfilment by location with tracking, cancellations, refunds, inventory per location |
| Customers | Accounts with email sign-in codes, addresses, marketing consent, data export and erasure |
| Staff | Roles and scopes, two-factor sign-in, invites, API keys |
| Email | Jinja templates with previews and test sends, sent over SMTP or Resend |
| Extending | Domain events and webhooks; apps installed over OAuth 2.1, answering extension points and adding admin blocks, pages and storefront elements; spores (Rust plugins compiled into the API) |
| AI agents | An MCP server for admin agents: any admin write can be previewed exactly, and agents can propose changes for staff to approve |
| Frontends | An admin panel and a storefront (Next.js on Vite, server-rendered), and a typed SDK for building your own |

## Where it runs

- **A server:** the native host (Rust, axum) on Postgres, deployed with Docker and Caddy
  (HTTPS) by Hypha's CLI.
- **Cloudflare Workers:** the same core compiled to WebAssembly, on Postgres through
  Hyperdrive, files in R2; the admin and storefront as Workers too.

## Try it locally

You need stable Rust (edition 2024) with the `wasm32-unknown-unknown` target, Node.js 24,
pnpm 12 and Docker.

```bash
git clone https://github.com/hyphacommerce/hypha && cd hypha
pnpm install
cp apps/admin/.env.example apps/admin/.dev.vars
cp apps/storefront/.env.example apps/storefront/.dev.vars

# The API (:3917), the admin (:3001) and the storefront (:3002), on a demo database.
HYPHA_DEV_DB=hypha_demo HYPHA_OWNER_EMAIL=owner@example.com \
  HYPHA_OWNER_PASSWORD=demo-owner-password pnpm dev

# In another terminal: a demo store with markets, products, stock, sales, discounts and
# orders. A store seeded before price lists and discounts gets them with `pnpm seed --pricing`.
pnpm seed
```

Sign in to the admin at http://localhost:3001 as `owner@example.com`. `pnpm seed` prints a
`STOREFRONT_KEY` and a `HYPHA_WEBHOOK_SECRET`: put them in `apps/storefront/.dev.vars` and
restart `pnpm dev` to shop at http://localhost:3002. Emails land in Mailpit at
http://localhost:8025.

## Deploy it

```bash
pnpm hypha init      # a deployment folder: target, domains, secrets
pnpm hypha deploy    # builds and starts it
```

`--target vps` deploys to a machine over SSH, `--target cloudflare` to Workers with one
API token, and `--target local` to this machine under `*.hypha.localhost`. Until the
first release, the CLI runs from a checkout of this repository.

## Build on it

- **Apps** run anywhere, in any language, and talk to Hypha over HTTP: OAuth 2.1 install,
  webhooks, extension points (payments, shipping rates, search), admin blocks and pages,
  storefront elements. See [`docs/apps`](docs/apps) and the SDK, `@hyphacommerce/app`.
  Examples: [app-stripe](https://github.com/hyphacommerce/app-stripe),
  [app-meilisearch](https://github.com/hyphacommerce/app-meilisearch).
- **Spores** are Rust crates compiled into the API, on the same contracts as apps. See
  [`docs/spores`](docs/spores) and the worked example,
  [spore-stripe](https://github.com/hyphacommerce/spore-stripe).
- **AI agents** connect over MCP, as a staff member's client or on their own. See
  [`docs/agents`](docs/agents).
- **Your own frontend:** the admin and storefront APIs are described in
  [`openapi/`](openapi), with typed clients in `@hyphacommerce/sdk`.

## How it's built

Hypha is a Cargo workspace of small crates, one per domain, in a hexagonal architecture:
hosts depend on domain crates, which depend on `hypha-core`, never the other way. The core
and every domain crate compile to WebAssembly and do no I/O of their own: storage, time,
IDs and HTTP come in through ports, so the same commands run on a server and on Workers,
and tests are fast and deterministic. Every operation is a command with typed input,
required scopes and typed errors, which is what lets one surface serve HTTP, MCP and the
CLI. The design notes in [`docs/design`](docs/design) explain each decision.

| Path | What |
| --- | --- |
| `crates/hypha-core`, `crates/hypha-<domain>` | Shared types and the domain crates (products, markets, pricing, cart, orders, payments…) |
| `crates/hypha-http` | The HTTP layer: routes, DTOs, OpenAPI, shared by both hosts |
| `crates/hypha-host-axum`, `crates/hypha-host-workers` | The native host and the Workers host |
| `crates/hypha-store-postgres`, `crates/hypha-store-s3` | Storage adapters |
| `apps/admin`, `apps/storefront` | The admin panel and the storefront |
| `packages/*` | The CLI, the SDK and the apps platform's packages |

## Coming next

- **Agentic shopping** through the [Universal Commerce Protocol](https://ucp.dev), with
  [ucphub.ai](https://ucphub.ai): a connector so shopping agents can browse and buy from a
  Hypha store.
- The CLI on npm (`npx @hyphacommerce/cli init`) and starters for the storefront and admin ([docs/releases.md](docs/releases.md)).
- Gift cards and store credit.

## Contributing

Bug reports, ideas and docs fixes are welcome; code changes start with an issue so we agree
on the shape first. [CONTRIBUTING.md](CONTRIBUTING.md) has the details and the checks a
change passes before it's done. Vulnerabilities go through [SECURITY.md](SECURITY.md),
never an issue.

## Sponsoring

Hypha is free and open source, built by one maintainer. If it saves you a platform fee or
an integration project, [sponsoring it on GitHub](https://github.com/sponsors/hyphacommerce)
is what keeps it moving: sponsors are named here and on
[hyphacommerce.com](https://hyphacommerce.com). Companies that want a feature, a
spore or an integration built should get in touch at hello@hyphacommerce.com.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed
as above, without any additional terms or conditions.
