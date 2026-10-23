---
title: Introducing Hypha
description: An open-source commerce platform you own, host and extend, grown as a network rather than rented as a product.
date: 2026-10-23
tags: [announcements]
draft: true
---

<!-- Starting copy from the Claude Design mock-up, checked against what Hypha does, for
Danilo to rewrite in his own words. It's a draft: it shows in `astro dev` and is left out
of builds until `draft: true` above is removed. -->

Today we're opening up Hypha: an open-source commerce platform with a small core written in
Rust, and everything else (apps, plugins, storage, hosting, frontends and AI agents)
connected around it.

## Why we built it

Most merchants rent their store. The rent starts small, then grows with every sale, every
app and every feature that sits behind a higher plan. The code, the data and the servers
belong to someone else, and leaving is expensive by design.

We wanted the opposite: a platform you can read, run on a server you choose, and change
without asking permission.

> Commerce software should be infrastructure you own, not a landlord you pay.

## A small core, everything plugs in

A hypha is a single thread of a fungus. On its own it does very little; connected into a
mycelium it feeds a forest. Hypha works the same way. The core handles products, markets,
carts, orders, payments and customers, and stays small enough to understand. Everything else
connects to it.

### Spores

Spores are Rust plugins compiled into the API. Use them when you need code that runs inside
Hypha itself, such as a payment provider with its own tables, routes and settings.

### Apps

Apps talk to Hypha over HTTP and can be written in any language. They install over OAuth,
receive webhooks, answer extension points like shipping rates and payments, and add blocks
and pages to the admin.

### Agents

AI agents connect over MCP. Staff decide how far each one goes: it can read, preview a
change exactly before making it, propose changes for a person to approve, or apply them
directly once it's trusted to.

## What it costs

Hypha is free. Running it costs whatever your server costs, and for most stores that is a
small VPS.

| Setup | Per month |
| --- | --- |
| Hypha on a small VPS | about €5 |
| Hypha on Cloudflare Workers | about $5 |
| A hosted platform's basic plan | $39, plus 2% of sales without its own payments |

<div class="note">
<strong>Hypha is early</strong>

It works end to end, but it isn't production-proven yet and its APIs may change between
0.x releases. Follow the [changelog](/changelog), and try it beside your current store
first.
</div>

## Try it

The code is on GitHub. To run it locally:

```bash
git clone https://github.com/hyphacommerce/hypha && cd hypha
pnpm install
pnpm dev
```

## What's next

- Agentic shopping with [ucphub.ai](https://ucphub.ai)
- Suppliers and purchase orders
- Discounts
- Analytics

Open an issue, or just tell us what you're building: hello@hyphacommerce.com.
