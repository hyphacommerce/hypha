// Connecting the storefront to the API, after the first start: a publishable key for
// the Online Store channel, and on a server the storefront registered as an app whose
// webhook refreshes its static pages (design note 0024, §7). What `pnpm seed` does
// locally, done once per deployment, as the owner.

import { type Staff } from "./api.ts";
import { type Deployment, url } from "./deployment.ts";

/** The events that change what a static page shows. */
const EVENTS = [
  "product.*",
  "inventory_item.*",
  "price_list.*",
  "catalog.*",
  "channel.*",
  "category.*",
  "market.*",
];

/** `webhookSecret` is `null` without a webhook (on this machine). */
export type StorefrontKeys = { key: string; webhookSecret: string | null };

/** Where the API should deliver the storefront's events. */
function webhookUrl(d: Deployment): string {
  return `${url(d, d.domains.storefront!)}/api/revalidate`;
}

/** Points the storefront's webhook at its current domain; answers the new URL when it
 * moved, `null` when it was already right. */
export async function syncWebhook(d: Deployment, staff: Staff): Promise<string | null> {
  const app = await staff.call<{ webhook: { url: string } | null }>(
    "GET",
    "/admin/apps/app_storefront",
  );
  const target = webhookUrl(d);
  if (app.webhook?.url === target) return null;
  await staff.call("PATCH", "/admin/apps/app_storefront", { webhook: { url: target } });
  return target;
}

/**
 * The app first: it's the call that can be refused (a webhook URL the API won't call),
 * and a refused one leaves nothing behind (on a server or on Cloudflare alike). On this
 * machine there's no webhook: the API
 * calls only https URLs (or localhost), and the storefront is reachable from its
 * container only as `http://storefront:3000`. Its static pages then refresh within
 * their five minutes.
 */
export async function connectStorefront(d: Deployment, staff: Staff): Promise<StorefrontKeys> {
  const channels = await staff.call<{ items: { id: string; kind: string }[] }>(
    "GET",
    "/admin/channels",
  );
  const channel = channels.items.find((c) => c.kind === "online_store");
  if (!channel) throw new Error("the store has no Online Store channel to sell through");
  let webhookSecret: string | null = null;
  if (d.target !== "local") {
    const app = await staff.call<{ secret: string }>("POST", "/admin/apps", {
      key: "app_storefront",
      name: "Storefront",
      subscriptions: EVENTS,
      webhook: { url: webhookUrl(d) },
    });
    webhookSecret = app.secret;
  }
  const key = await staff.call<{ token: string }>("POST", "/admin/api-keys", {
    kind: "publishable",
    name: "Storefront",
    channel: channel.id,
  });
  return { key: key.token, webhookSecret };
}
