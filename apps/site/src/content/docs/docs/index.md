---
title: Introduction
description: What Hypha is, what it includes and where to start.
---

Hypha is an open-source commerce API written in Rust, with an admin panel and a storefront
built on it. It runs on your own server or on Cloudflare Workers, grows through apps and
Rust plugins instead of forks, and lets AI agents operate a store with exact previews and
approvals.

:::caution[Public beta]
Hypha works end to end: catalog, checkout, payments, fulfilment, accounts, apps and agents.
It hasn't carried a real shop's orders yet, and its APIs may change between betas. Each
change is in the [changelog](/changelog); see [Releases and upgrades](/docs/releases/).
:::

## What's included

| Area | What you get |
| --- | --- |
| Catalog | Products with options and sparse, explicit variants, categories, media, rich text, custom fields, search with facets |
| Selling | Markets (countries and currency), catalogs, sales channels, tax regions and classes |
| Prices | Price lists that compete on the lowest price: sales and clearances on a schedule, compare-at prices, quantity breaks, prices for customer groups, a spreadsheet editor with CSV import |
| Discounts | Codes and automatic discounts: amounts or percentages off products, the order or shipping, buy X get Y, volume tiers, combinations, usage limits |
| Checkout | Carts priced per market, shipping zones and rates (or a rate app), manual payment methods and online payments started before the order exists |
| Orders | Fulfilment by location with tracking, cancellations, refunds, inventory per location |
| Customers | Accounts with email sign-in codes, addresses, marketing consent, data export and erasure |
| Staff | Roles and scopes, two-factor sign-in, invites, API keys |
| Email | Jinja templates with previews and test sends, sent over SMTP or Resend |
| Extending | Events and webhooks, apps installed over OAuth 2.1, spores (Rust plugins compiled in) |
| AI agents | An MCP server for the admin, with exact previews of any write and proposals staff approve |

## How the pieces fit

The **API** is the product: two HTTP surfaces, described by OpenAPI documents.

- The **admin API** (`/admin/*`) is for staff sessions, secret API keys, installed apps and
  agents.
- The **storefront API** (`/storefront/*`) is for publishable keys and signed-in shoppers.
  It never shows drafts or admin-only fields.

The **admin panel** and the **storefront** are applications on those APIs, written in
TypeScript (Next.js on Vite). You can run them as they are, start your own from them, or
build a frontend from scratch on the typed SDK, `@hyphacommerce/sdk`.

Everything you add goes through the same commands the core uses. Every operation has typed
input, the scopes it requires and typed errors, so an app, a plugin, an agent or your own
frontend can't skip a rule the core enforces.

## What's published

Every release ships as one set with one version: the source, Docker images for the API,
the admin and the storefront (`ghcr.io/hyphacommerce/hypha-*`), and the npm packages
under `@hyphacommerce/`: the `cli` that deploys it, the `sdk` for your own frontends, and
`app`, `app-bridge`, `app-bridge-host`, `app-blocks` and `storefront-elements` for app
authors. During the beta they're on npm's `next` tag.

## Where to start

- [Deploy it](/docs/deploy/): `npx @hyphacommerce/cli@next init`, then `deploy`, to your
  own server, this machine or Cloudflare Workers. No Rust toolchain needed for a server.
- [Run it locally](/docs/getting-started/): the API, admin and storefront from the source,
  with a demo store, for working on Hypha itself.
- [Concepts](/docs/concepts/stores/): how stores, markets, products, orders and payments
  work.
- [Build an app](/docs/guides/apps/), [write a spore](/docs/guides/spores/) or
  [connect an AI agent](/docs/guides/agents/).
- The [admin API](/docs/api/admin/) and [storefront API](/docs/api/storefront/) references.
