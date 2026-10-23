---
title: Staff, scopes and keys
description: Who can call the API, and what each caller may do.
sidebar:
  order: 8
---

Every request to the admin API acts as someone: a staff member, an API key, an installed
app or an AI agent. What it may do is a set of **scopes**, and every command checks the
scopes it needs before it does anything.

## Scopes

Scopes are named after an area and an access: `products:read`, `orders:write`,
`customers:erase`. A write scope includes its read scope. Personal data about customers
(names, emails, phones, addresses) needs `customer_data:*` scopes on top: a caller without
them gets those fields left out, and the response lists what was left out in `redacted`.

## Staff

Staff members sign in with a password and, optionally, a second factor (an authenticator
app, with recovery codes). Each has a **role**, a named set of scopes; the owner's role has
every scope. Staff invite others by email, and nobody can grant a scope they don't hold
themselves.

## API keys

- **Secret keys** (`hypha_sk_…`) are for servers: scripts, integrations, a backend. They
  carry the scopes they were created with.
- **Publishable keys** (`hypha_pk_…`) are for storefronts. They reach only the storefront
  API, for one [sales channel](/docs/concepts/channels/).

Keys are stored only as hashes, and shown once when created.

## Apps and agents

Installed apps act with the scopes staff approved when installing them. AI agents act with
their own grant, and a connected agent never exceeds the role of the person it acts for.
See [Build an app](/docs/guides/apps/) and [Connect an AI agent](/docs/guides/agents/).

## Shoppers

Shoppers sign in to the storefront with a six-digit code sent to their email; there are no
passwords to leak. A signed-in shopper reaches their own orders, addresses, marketing
choices and data, and nothing else.
