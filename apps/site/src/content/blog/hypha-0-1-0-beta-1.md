---
title: "Hypha 0.1.0 beta 1: the first public release"
description: Hypha is now public. Rust commerce API, admin, storefront and CLI, as source, npm packages and Docker images, running on a server or on Cloudflare Workers.
date: 2026-10-25
tags: [releases]
release: true
version: 0.1.0-beta.1
changes:
  - kind: Added
    text: Six npm packages on the `next` tag, `@hyphacommerce/sdk` among them, with provenance.
  - kind: Added
    text: Multi-arch images `ghcr.io/hyphacommerce/hypha-api`, `hypha-admin` and `hypha-storefront`.
  - kind: Added
    text: The API's Workers bundle attached to the GitHub release.
  - kind: Changed
    text: The repository is public, under MIT or Apache-2.0.
---

Hypha has been a private repository for about a month. Today it's public, tagged
`v0.1.0-beta.1`, and anyone can read it, run it or build on it.

## What Hypha is

Hypha is a commerce API written in Rust, with an admin panel and a storefront built
on Next.js. The same core runs on a plain server. Compiled to WebAssembly, it also runs on
Cloudflare Workers, with Postgres through Hyperdrive and files in R2.

The catalogue is built for large and awkward product ranges. Variants are explicit, so a
product with twelve options doesn't turn into thousands of combinations nobody sells.
Markets set the country and currency, price lists compete on the lowest price, and
discounts cover codes, automatic rules, buy X get Y and volume tiers. Checkout starts an
online payment before the order exists, and manual methods such as bank transfer still
work. Customers sign in with an emailed code; there are no passwords on the storefront.

You extend it from outside rather than by forking. Apps connect over HTTP with OAuth and
answer extension points such as shipping rates and payments. Spores are Rust plugins
compiled into the API, which is how the Stripe integration ships. There's also an MCP
server for admin agents. Any write an agent wants to make can be previewed exactly, and an
agent can leave a proposal for a person to approve.

## What a beta means here

It works end to end, and the demo at [demo.hyphacommerce.com](https://demo.hyphacommerce.com)
runs the real thing. It hasn't carried a real shop's orders yet, so the HTTP API, event
payloads and configuration keys may still change between betas. Each change is listed in
the [changelog](/changelog). Migrations are append-only, so a database made by one beta
upgrades to the next.

The beta ends at `0.1.0`, once a few shops have run on it.

## What's published

- The source at [github.com/hyphacommerce/hypha](https://github.com/hyphacommerce/hypha),
  MIT or Apache-2.0 at your option.
- npm packages under the `next` tag: `@hyphacommerce/sdk` (typed clients for both API
  surfaces), `@hyphacommerce/app` (the SDK for app authors), `app-bridge`,
  `app-bridge-host`, `app-blocks` and `storefront-elements`.
- Docker images for amd64 and arm64: `ghcr.io/hyphacommerce/hypha-api:next`,
  `hypha-admin:next` and `hypha-storefront:next`.
- The API's Workers bundle, attached to the
  [release](https://github.com/hyphacommerce/hypha/releases/tag/v0.1.0-beta.1).

The CLI, `@hyphacommerce/cli`, isn't on npm yet. It still builds the images from a
checkout, and the next beta teaches it to pull the published ones instead.

## Trying it

With Rust, Node 24, pnpm and Docker installed:

```bash
git clone https://github.com/hyphacommerce/hypha && cd hypha
pnpm install
HYPHA_DEV_DB=hypha_demo HYPHA_OWNER_EMAIL=owner@example.com \
  HYPHA_OWNER_PASSWORD=demo-owner-password pnpm dev
pnpm seed   # in another terminal: a demo store with products, stock, sales and orders
```

The admin is at `localhost:3001`, the storefront at `localhost:3002`, and the
[docs](/docs) cover deploying to a server or to Cloudflare.

## Where it's going

The next betas are about the CLI running from published images, so `npx @hyphacommerce/cli init`
works without a Rust toolchain, then starters for the storefront and admin. Agentic
shopping through the Universal Commerce Protocol is planned after that.

If you try it on a real catalogue, I'd like to hear what was missing. Issues and
discussions on GitHub are the place. The project is built by one person, so
[sponsoring it](https://github.com/sponsors/hyphacommerce) is what keeps it moving.
