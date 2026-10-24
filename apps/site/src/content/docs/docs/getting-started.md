---
title: Run it locally
description: The API, the admin panel and the storefront on your machine, with a demo store.
---

One command starts the API, the admin panel and the storefront, with the database, file
storage and a mail catcher in Docker. A second fills the store with demo data.

## What you need

- Stable Rust (edition 2024) with the WebAssembly target:
  `rustup target add wasm32-unknown-unknown`
- Node.js 24 and pnpm 12
- Docker (Docker Desktop on macOS and Windows)

The first build compiles the whole API, which takes a few minutes and a few gigabytes of
disk.

## Start it

```bash
git clone https://github.com/hyphacommerce/hypha && cd hypha
pnpm install
cp apps/admin/.env.example apps/admin/.dev.vars
cp apps/storefront/.env.example apps/storefront/.dev.vars

HYPHA_DEV_DB=hypha_demo HYPHA_OWNER_EMAIL=owner@example.com \
  HYPHA_OWNER_PASSWORD=demo-owner-password pnpm dev
```

`pnpm dev` starts the compose services (Postgres, an S3-compatible file server, Mailpit),
then:

| What | Where |
| --- | --- |
| The API | http://127.0.0.1:3917 |
| The admin panel | http://localhost:3001 |
| The storefront | http://localhost:3002 |
| Emails the store sends | http://localhost:8025 (Mailpit) |

The owner account is created on the first start from `HYPHA_OWNER_EMAIL` and
`HYPHA_OWNER_PASSWORD`. Ctrl+C stops everything.

## Fill it with a demo store

In another terminal:

```bash
pnpm seed
```

The seed works through the admin and storefront APIs the way a merchant and their shoppers
would: store details, markets with taxes and prices, warehouses, shipping zones,
categories, products with options and stock, a sale, a clearance and wholesale prices,
discounts (the `WELCOME10` code, free shipping, buy a grow kit get a syringe free, a
volume tier), a publishable key, payment methods, and a few orders. It stops if the store already has products.

It prints a `STOREFRONT_KEY` and a `HYPHA_WEBHOOK_SECRET`. Put both in
`apps/storefront/.dev.vars`, then restart `pnpm dev`: the storefront can now show the
catalog, and product changes refresh its pages at once.

## Sign in

Open the admin at http://localhost:3001 and sign in as `owner@example.com` with
`demo-owner-password`. From there, browse the products, place an order on the storefront
and fulfil it.

## Next

- [Deploy it](/docs/deploy/) to a server or to Cloudflare Workers.
- Read how [markets, catalogs and price lists](/docs/concepts/markets/) decide what a
  shopper sees and pays, then [prices and sales](/docs/concepts/pricing/) and
  [discounts](/docs/concepts/discounts/).
