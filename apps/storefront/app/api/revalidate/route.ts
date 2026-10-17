// Hypha's webhook for the storefront's static pages (design note 0024, §7). Register
// the storefront as an app in the admin (Settings → Apps) with this URL as its webhook
// and the product, inventory, price, catalog, channel, category and market events, and
// put its signing secret in `HYPHA_WEBHOOK_SECRET`. Each event expires the tags of the
// cached reads it affects (`lib/cache-tags.ts`); the pages that rendered them are
// re-rendered on their next visit. Nothing is warmed here: a page nobody visits isn't
// rendered for nothing.
//
// Deliveries are signed like Stripe's (`Hypha-Signature: t=<unix seconds>,v1=<hex
// HMAC-SHA256 of "<t>.<body>">`, `outbound.rs`) and retried until a 2xx; this answers
// non-2xx for anything it didn't act on, so a wrong secret shows up as failed
// deliveries in the admin instead of silently stale pages.

import { revalidatePath, revalidateTag } from "next/cache";
import { type NextRequest, NextResponse } from "next/server";

import { api, loadCountries } from "@/lib/api";
import { TAGS } from "@/lib/cache-tags";
import { link } from "@/lib/site";

/** How far a delivery's timestamp may be from now: older ones are replays. */
const TOLERANCE_SECONDS = 5 * 60;

/** Products resolved one by one before giving up and expiring every product page. */
const MAX_RESOLVED = 20;

type Envelope = {
  id: string;
  type: string;
  subject: { type: string; id: string };
  data: Record<string, unknown>;
};

/**
 * Deliveries already acted on, by `Hypha-Delivery-Id`: a retry after a lost answer
 * shouldn't expire everything again. Per instance and bounded; at worst a retry acts
 * twice, which is harmless.
 */
const seen = new Map<string, number>();
const SEEN_MS = 10 * 60 * 1000;
const SEEN_MAX = 5_000;

function alreadySeen(delivery: string | null): boolean {
  if (!delivery) return false;
  const now = Date.now();
  if (seen.size > SEEN_MAX) {
    for (const [key, until] of seen) if (until <= now) seen.delete(key);
  }
  const until = seen.get(delivery);
  if (until && until > now) return true;
  seen.set(delivery, now + SEEN_MS);
  return false;
}

/** Whether `header` signs `body` under `secret`, recently. */
async function verified(secret: string, header: string, body: string): Promise<boolean> {
  const parts = Object.fromEntries(
    header.split(",").map((part) => {
      const [key, ...value] = part.trim().split("=");
      return [key, value.join("=")];
    }),
  );
  const at = Number(parts.t);
  const given = parts.v1 ?? "";
  if (!Number.isInteger(at) || Math.abs(Date.now() / 1000 - at) > TOLERANCE_SECONDS) return false;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(`${at}.${body}`));
  const expected = Array.from(new Uint8Array(mac), (b) => b.toString(16).padStart(2, "0")).join("");
  // The length isn't secret; the comparison of the bytes is constant-time.
  if (given.length !== expected.length) return false;
  let difference = 0;
  for (let i = 0; i < expected.length; i++) difference |= given.charCodeAt(i) ^ expected.charCodeAt(i);
  return difference === 0;
}

const ids = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];

/**
 * The product pages to expire for products by ID: each one's handle when the storefront
 * can still read it (published and active), else every product page (an unpublished
 * product's page must go, and its handle can't be read any more).
 */
async function productTags(products: string[]): Promise<string[]> {
  if (products.length === 0) return [];
  if (products.length > MAX_RESOLVED) return [TAGS.products];
  const [country] = await loadCountries();
  if (!country) return [TAGS.products];
  const handles = await Promise.all(
    products.map(async (id) => {
      const { data } = await api().GET("/storefront/products/{id}", {
        params: { path: { id }, query: { country: country.country } },
      });
      return data?.handle;
    }),
  );
  return handles.every(Boolean) ? handles.map((h) => TAGS.product(h as string)) : [TAGS.products];
}

/** What an event changes, as tags (and paths, for a page that may be cached as a 404). */
async function affected(event: Envelope): Promise<{ tags: string[]; paths: string[] }> {
  const data = event.data;
  switch (event.type) {
    case "product.created": {
      // Its page may have been visited before it existed: a cached 404 carries no tags.
      const handle = typeof data.handle === "string" ? data.handle : undefined;
      const countries = handle ? await loadCountries() : [];
      return {
        tags: [TAGS.listings, ...(handle ? [TAGS.product(handle)] : [])],
        paths: countries.map((c) => link(c.country, `/p/${handle}`)),
      };
    }
    case "product.updated":
    case "product.variant_added":
    case "product.variant_removed":
    case "product.variant_updated":
    case "product.tax_class_set":
      return { tags: [TAGS.listings, ...(await productTags([event.subject.id]))], paths: [] };
    case "catalog.products_added":
    case "catalog.products_removed":
    case "channel.products_published":
    case "channel.products_unpublished":
    case "category.products_added":
    case "category.products_removed":
      return { tags: [TAGS.listings, ...(await productTags(ids(data.products)))], paths: [] };
    // Stock and variant prices name variants, not products: every product page.
    case "inventory_item.changed":
    case "inventory_item.deleted":
    case "price_list.prices_set":
    case "price_list.prices_removed":
      return { tags: [TAGS.listings, TAGS.products], paths: [] };
    case "price_list.changed":
    case "market.changed":
    case "catalog.changed":
      return { tags: [TAGS.prices, TAGS.store], paths: [] };
    case "category.changed":
    case "category.deleted":
      return { tags: [TAGS.categories, TAGS.listings, ...(await productTags(ids(data.products)))], paths: [] };
    default:
      return { tags: [], paths: [] };
  }
}

export async function POST(request: NextRequest) {
  const secret = process.env.HYPHA_WEBHOOK_SECRET;
  if (!secret) {
    console.error("HYPHA_WEBHOOK_SECRET is not set: static pages won't follow changes");
    return NextResponse.json({ ok: false, error: "HYPHA_WEBHOOK_SECRET is not set" }, { status: 500 });
  }
  const body = await request.text();
  const signature = request.headers.get("hypha-signature") ?? "";
  if (!(await verified(secret, signature, body))) {
    return NextResponse.json({ ok: false, error: "invalid signature" }, { status: 401 });
  }
  let event: Envelope;
  try {
    event = JSON.parse(body);
  } catch {
    return NextResponse.json({ ok: false, error: "invalid payload" }, { status: 400 });
  }
  if (alreadySeen(request.headers.get("hypha-delivery-id"))) {
    return NextResponse.json({ ok: true, repeated: true });
  }

  const { tags, paths } = await affected(event);
  // `{ expire: 0 }`: the next visit renders fresh, rather than once more from the stale
  // copy (what "max" would do), which would read as "the change didn't show".
  for (const tag of new Set(tags)) revalidateTag(tag, { expire: 0 });
  for (const path of paths) revalidatePath(path);
  return NextResponse.json({ ok: true, type: event.type, tags: [...new Set(tags)], paths });
}
