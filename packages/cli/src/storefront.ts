// Connecting the storefront to the API, after the first start: a publishable key for
// the Online Store channel, on a server the storefront registered as an app whose
// webhook refreshes its static pages (design note 0024, §7), and the emails' order links
// pointed at it (design note 0028, §2). What `pnpm seed` does locally, done once per
// deployment, as the owner.

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
  // Custom field definitions: one shown to storefronts, or no longer, changes product
  // and category pages (design note 0033, §6.2).
  "custom_field_definition.*",
];

/** `webhookSecret` is `null` without a webhook (on this machine); `orderUrl` is the
 * emails' order links when this set them, `null` when the store already had its own. */
export type StorefrontKeys = { key: string; webhookSecret: string | null; orderUrl: string | null };

/** What the setup needs of a staff session (a fake one in tests). */
type Calls = Pick<Staff, "call">;

/**
 * The storefront's order link for emails (`order_url`): `app/order/[id]/route.ts` keeps
 * the token in the buyer's cookie and opens the confirmation in the order's country, in
 * either routing mode.
 */
export function orderUrl(d: Deployment): string {
  return `${url(d, d.domains.storefront!)}/order/{order}?token={token}`;
}

/** Points the emails' order links at the storefront unless the store has its own;
 * answers the pattern it set, `null` when it kept the store's. */
export async function linkOrders(d: Deployment, staff: Calls): Promise<string | null> {
  const settings = await staff.call<{ order_url: string | null }>(
    "GET",
    "/admin/notification-settings",
  );
  if (settings.order_url) return null;
  const pattern = orderUrl(d);
  await staff.call("PATCH", "/admin/notification-settings", { order_url: pattern });
  return pattern;
}

/** Where the API should deliver the storefront's events. */
function webhookUrl(d: Deployment): string {
  return `${url(d, d.domains.storefront!)}/api/revalidate`;
}

/** Points the storefront's webhook at its current domain; answers the new URL when it
 * moved, `null` when it was already right. */
export async function syncWebhook(d: Deployment, staff: Calls): Promise<string | null> {
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
 * The order links first, then the app: a second run after a failure finds the links set
 * and keeps them, and the app is the call that can be refused (a webhook URL the API
 * won't call), which leaves nothing behind (on a server or on Cloudflare alike). On this
 * machine there's no webhook: the API
 * calls only https URLs (or localhost), and the storefront is reachable from its
 * container only as `http://storefront:3000`. Its static pages then refresh within
 * their five minutes.
 */
export async function connectStorefront(d: Deployment, staff: Calls): Promise<StorefrontKeys> {
  const channels = await staff.call<{ items: { id: string; kind: string }[] }>(
    "GET",
    "/admin/channels",
  );
  const channel = channels.items.find((c) => c.kind === "online_store");
  if (!channel) throw new Error("the store has no Online Store channel to sell through");
  const linked = await linkOrders(d, staff);
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
  return { key: key.token, webhookSecret, orderUrl: linked };
}
