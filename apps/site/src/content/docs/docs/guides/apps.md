---
title: Build an app
description: Apps run anywhere, in any language, and extend Hypha over HTTP.
sidebar:
  order: 2
---

An **app** is a service you run anywhere, in any language, that a store installs. It can
read and write through the admin API, receive webhooks, answer Hypha mid-flow (payments,
shipping rates, search), and add blocks and pages to the admin and elements to the
storefront. Payment providers, search engines, carriers, ERPs and marketplaces are apps.

Choose an app for anything a third party runs as a service, anything not written in Rust,
or anything a store should install without rebuilding its API. For code compiled into the
API itself, see [spores](/docs/guides/spores/).

## Start from the starter

```bash
npx @hyphacommerce/cli@next create app my-app
cd my-app && pnpm install && pnpm test
APP_URL=http://localhost:8787 pnpm dev
```

The starter is a small Hono app on `@hyphacommerce/app`: the manifest, the install flow,
a webhook handler, one extension handler (`shipping.rates`, quoting a flat rate) and
Node and Workers entries. Install it from your admin (Apps → Install app →
`http://localhost:8787`; a local API needs `outbound.allow_private = true` to call it),
then change the manifest and the handlers to yours. The rest of this guide is what each
part does.

## The manifest

An app describes itself in a JSON **manifest**, served at a public URL. Its URL is the
app's OAuth client ID.

```json
{
  "key": "app_reviews",
  "name": "Reviews",
  "version": "1.0.0",
  "app_url": "https://reviews.example.com",
  "redirect_uris": ["/oauth/callback"],
  "privacy_url": "https://reviews.example.com/privacy",
  "scopes": ["products:read", "orders:read"],
  "webhooks": { "url": "/hypha/webhooks", "events": ["order.placed", "product.*"] },
  "extension_points": [],
  "admin": {
    "blocks": [{ "id": "summary", "target": "product.details", "url": "/hypha/blocks/product" }]
  }
}
```

Relative URLs resolve against `app_url`. Hypha checks every rule and reports each problem
with its path; the [JSON Schema](https://github.com/hyphacommerce/hypha/blob/main/docs/apps/manifest.schema.json)
lets an editor check it as you type. Besides webhooks and blocks, a manifest can declare
`extension_points`, admin `pages` and `actions`, storefront `elements`, `custom_fields` of
its own, and `settings` staff fill in.

## Installing: OAuth 2.1

1. Staff click **Install app** in the admin, which opens your app's
   `/install?api=<the store's API>`.
2. Your app sends the browser to the store's `/oauth/authorize` with PKCE. Staff see the
   app's name, its scopes and why it wants each one, and approve.
3. Your app exchanges the code for tokens: an access token for an hour and a rotating
   refresh token for 30 days. The first exchange also hands over the app's **signing
   secret**, once.

`@hyphacommerce/app` (`pnpm add @hyphacommerce/app@next`) does all of it:
`createAuthorizationUrl`, `exchangeCode`, and a `HyphaClient` that refreshes tokens on its
own (once, even across processes sharing an installation) and is typed with the admin
API's paths.

## Webhooks

Hypha sends events from its outbox, signed with your app's secret (`Hypha-Signature`), with
retries on a schedule. Each delivery carries the event's ID and its own (`Hypha-Event-Id`,
`Hypha-Delivery-Id`); give `webhookHandler` a `dedupe` store to drop repeats.

```ts
import { webhookHandler } from "@hyphacommerce/app";

export default webhookHandler({
  // One secret per installation: look it up by the store the delivery comes from.
  secret: async ({ store }) => secretFor(store),
  async onEvent(event) {
    if (event.type === "order.placed") await thankBuyer(event.subject.id);
  },
});
```

An app only gets events about what its scopes let it read, and personal data in them only
with the matching `customer_data:*` scopes.

## Extension points

An extension point is a synchronous call Hypha makes to your app in the middle of a flow,
with a typed request and a typed answer, a timeout and what happens when the app fails:

| Point | When |
| --- | --- |
| `payments.start`, `payments.status`, `payments.capture`, `payments.void`, `payments.refund` | A cart's payment and what staff do with it |
| `shipping.rates` | Quoting shipping for a cart |
| `search.query` | A storefront search, when the store uses the app's engine |
| `admin.action` | A button the app added to the admin |

```ts
import { defineExtensionHandler, extensionHandler } from "@hyphacommerce/app";

const rates = defineExtensionHandler("shipping.rates", async (request) => ({
  rates: [
    { service: "express", name: "Express", amount: { amount: 990, currency: request.currency } },
  ],
}));

export default extensionHandler({ handlers: [rates], secret: SECRET });
```

The types come from the [extension points reference](/docs/api/extensions/). Every call
is logged, and staff can see each app's calls in the admin.

## The admin and the storefront

- **Blocks** are small UI trees your app returns as JSON (a documented format, rendered
  by the admin with its own components) on targets such as an order's payment card.
- **Pages** are full pages of yours, embedded in the admin and talking to it through the
  app bridge (`@hyphacommerce/app-bridge`), with session tokens to know which staff member
  is looking.
- **Storefront elements** are web components a storefront loads, such as a payment form,
  mounted with `@hyphacommerce/storefront-elements`.

## Examples

- [app-stripe](https://github.com/hyphacommerce/app-stripe): payments through the
  `payments.*` points, a block on orders and a card form on the storefront.
- [app-meilisearch](https://github.com/hyphacommerce/app-meilisearch): the storefront's
  search on Meilisearch, through `search.query`.
