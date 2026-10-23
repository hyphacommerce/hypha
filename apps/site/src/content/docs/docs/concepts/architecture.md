---
title: Architecture
description: A small core, domain crates around it, and everything else plugged in through ports.
sidebar:
  order: 0
---

Hypha is a Cargo workspace of small crates in a hexagonal (ports and adapters)
architecture. Dependencies point inward: hosts depend on domain crates, and domain crates
depend on `hypha-core`, never the other way round.

## The layers

| Layer | What lives there |
| --- | --- |
| Core | Shared types only: money, prefixed IDs, countries, emails, scopes, the request context, generic ports (clock, IDs, batches) |
| Leaf domains | One crate per area: products, markets, pricing, tax, inventory, customers, orders, fulfilment, payments, media, search, apps, agents… |
| Composing domains and workflows | What spans several domains: carts, checkout, order actions, event dispatch, search indexing, email, privacy |
| Adapters | Storage behind the domains' ports: Postgres for data, S3 or R2 for files, SMTP or Resend for email |
| Hosts | The HTTP layer and the runtimes: a native server on axum, or WebAssembly on Cloudflare Workers |

The core and every domain crate compile to WebAssembly and do no I/O of their own. Time,
IDs, randomness, storage and HTTP come in through ports, so the same commands run on a
server and on Workers, and their tests are fast and deterministic.

## Commands

Every operation is a command: typed input, the scope it requires, and typed errors with
stable codes. Authorisation lives in commands; authentication lives in hosts. Because the
same command serves a staff member in the admin, an app over OAuth and an agent over MCP,
one rule applies everywhere.

## Ports and adapters

Each domain declares a repository trait per aggregate. An adapter implements them, and a
shared contract test suite says what every adapter must do: round trips, versions, unique
keys, batched writes. Postgres is the first adapter; another database is a crate that
passes the same suite.

## Extending without forking

- **Domain events** are written in the same transaction as the change, and delivered to
  in-process handlers and to apps' webhooks.
- **Extension points** are synchronous contracts Hypha calls mid-flow: payments, shipping
  rates, search.
- **Apps** run anywhere, in any language, and connect over HTTP. See the
  [apps guide](/docs/guides/apps).
- **Spores** are Rust crates compiled into the API, on the same contracts as apps. See the
  [spores guide](/docs/guides/spores).
- **Custom fields** add typed data to products, variants, categories, customers and orders.

The design notes in the repository's
[`docs/design`](https://github.com/hyphacommerce/hypha/tree/main/docs/design) explain each
decision.
