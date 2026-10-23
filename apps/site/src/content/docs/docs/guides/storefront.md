---
title: Build a storefront
description: Start from Hypha's storefront, or build your own on the storefront API and the typed SDK.
sidebar:
  order: 1
---

A storefront talks to the **storefront API** with a [publishable
key](/docs/concepts/channels/#publishable-keys). You have two ways to start.

## Start from Hypha's storefront

`apps/storefront` in the repository is a complete shop: country-aware URLs, a cached
catalog that refreshes when products change, search with facets, the cart, checkout with
online payments, customer accounts and order pages. It's a Next.js app built with Vite,
running on Node or on Cloudflare Workers.

Copy it and make it yours: its look lives in one theme file, and every page is ordinary
React. A `create` command that copies it out of the repository for you comes with the first
release.

## Build your own with the SDK

`@hyphacommerce/sdk` has typed clients generated from the API's OpenAPI documents: every
path, parameter and response is checked by TypeScript.

```ts
import { createStorefrontClient } from "@hyphacommerce/sdk";

const shop = createStorefrontClient("https://api.your-shop.example", "hypha_pk_…");

// Products for shoppers in Croatia, priced in that market's currency.
const { data: results } = await shop.GET("/storefront/search", {
  params: { query: { country: "HR", q: "jacket" } },
});

// A cart in that market, with two of a variant.
const { data: cart } = await shop.POST("/storefront/carts", { body: { country: "HR" } });
await shop.POST("/storefront/carts/{id}/lines", {
  params: { path: { id: cart!.id } },
  body: { variant: "var_…", quantity: 2 },
});
```

Publishable keys aren't secrets, so the client can run in the browser. Hypha's own
storefront still keeps it on its server and renders pages there, which keeps the cart ID in
an httpOnly cookie and the shop fast on slow devices.

## What a checkout takes

1. Create the cart in the shopper's country, and add lines.
2. Set the email and the shipping address, then pick a shipping option for each group of
   lines (`/shipping-options`, `/shipping/{group_id}`).
3. List the payment methods offered for the cart and set one.
4. For an online method, start the payment (`POST /payment`) and show what the provider
   asks for, such as a card form from the provider's app.
5. Check out (`POST /checkout`). The answer is the order, with an access token for its
   confirmation page.

Every step answers with the priced cart, including any problems per line (out of stock,
no longer sold in the market) for the shopper to fix. The [storefront API
reference](/docs/api/storefront/) lists every route.

## Keep cached pages fresh

Register your storefront as an app with a webhook, subscribed to product, inventory, price,
catalog and category events. Verify each delivery's signature, then expire the pages it
names. Hypha's storefront does this at `/api/revalidate`.
