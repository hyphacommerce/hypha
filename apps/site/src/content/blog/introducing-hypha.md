---
title: Introducing Hypha
description: An open-source commerce API in Rust, built to be extended, and safe for AI agents to operate.
date: 2026-10-23
tags: [announcement]
draft: true
---

<!-- An outline for Danilo to finish. It's a draft: it shows in `astro dev` and is left out
of builds until `draft: true` above is removed. -->

## Why another commerce platform

- What was missing: complicated catalogs, extensibility without forking, a host you
  choose.
- What Hypha is today, honestly: a public preview that works end to end.

## What's in it

- The catalog: options with sparse variants, custom fields, markets, price lists, channels.
- Checkout to fulfilment: payments taken before the order exists, fulfilment by location.
- The admin panel and the storefront, on a typed SDK.

## Extending it

- Apps, in any language, over OAuth, webhooks and extension points.
- Spores, Rust crates compiled into the API; the Stripe spore as the worked example.

## Agents that ask first

- Exact previews of any admin write.
- Proposals staff approve, and why that matters for prompt injection.

## Where it runs

- A server with the CLI; Cloudflare Workers with one API token.

## What's next

- The first release, v0.1.0.
- Agentic shopping through the Universal Commerce Protocol, with ucphub.ai.
- How to follow along, and sponsoring.
