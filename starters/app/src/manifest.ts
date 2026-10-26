// The app's manifest, served at `/manifest.json`: that URL is the app's OAuth client ID,
// and what staff see when they install it. Change the key, the name and the scopes to
// yours; relative URLs resolve against `app_url`.

import { type AppManifest, MANIFEST_SCHEMA_URL } from "@hyphacommerce/app";

/** `app_<name>`: lowercase letters, digits and `_`. Unique among the apps a store installs. */
export const APP_KEY = "app_starter";

/** What the app may read and write: each admin route's `x-hypha-scopes` in the API reference. */
export const SCOPES = ["orders:read", "shipping:read"] as const;

export const PATHS = {
  manifest: "/manifest.json",
  install: "/install",
  callback: "/hypha/oauth/callback",
  webhooks: "/hypha/webhooks",
  extensions: "/hypha/extensions",
  privacy: "/privacy",
} as const;

export function manifest(appUrl: string): AppManifest {
  return {
    $schema: MANIFEST_SCHEMA_URL,
    key: APP_KEY,
    name: "Starter",
    version: "0.1.0",
    app_url: appUrl,
    redirect_uris: [PATHS.callback],
    scopes: [...SCOPES],
    scope_reasons: {
      "orders:read": "Logs each order that's placed",
      "shipping:read": "Quotes a flat shipping rate at checkout",
    },
    privacy_url: PATHS.privacy,
    publisher: "You",
    webhooks: { url: PATHS.webhooks, events: ["order.placed", "app.uninstalled"] },
    extension_points: [{ point: "shipping.rates", version: 1, url: PATHS.extensions }],
  };
}
