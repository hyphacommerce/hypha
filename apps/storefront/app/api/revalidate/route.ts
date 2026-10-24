// Hypha's webhook for the storefront's static pages. Register the storefront as an app
// in the admin (Settings → Apps) with this URL as its webhook and the product,
// inventory, price, catalog, channel, category, market, custom_field_definition and
// discount events, and put its signing secret in `HYPHA_WEBHOOK_SECRET`. Each event
// expires the tags of the cached reads it affects (`lib/cache-tags.ts`); the pages that
// rendered them are re-rendered on their next visit. Nothing is warmed here: a page
// nobody visits isn't rendered for nothing.
//
// Deliveries are signed like Stripe's (`Hypha-Signature: t=<unix seconds>,v1=<hex
// HMAC-SHA256 of "<t>.<body>">`, `outbound.rs`) and retried until a 2xx; this answers
// non-2xx for anything it didn't act on, so a wrong secret shows up as failed
// deliveries in the admin instead of silently stale pages.

import { revalidatePath, revalidateTag } from "next/cache";
import { verifySignature } from "@hyphacommerce/app/signature";
import { type NextRequest, NextResponse } from "next/server";

import { api, loadCategories, loadCountries } from "@/lib/api";
import { TAGS } from "@/lib/cache-tags";
import { Deliveries } from "@/lib/deliveries";
import { link } from "@/lib/site";

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
 * shouldn't expire everything again.
 */
const deliveries = new Deliveries();

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
  const found = handles.filter((h): h is string => Boolean(h));
  return found.length === handles.length ? found.map((h) => TAGS.product(h)) : [TAGS.products];
}

type CategoryNode = { id: string; handle: string; children: CategoryNode[] };

/** A shown category's page by ID, else every category page (a hidden one's handle
 * can't be read). */
async function categoryTags(id: string | undefined): Promise<string[]> {
  if (!id) return [TAGS.categories];
  const find = (nodes: CategoryNode[]): CategoryNode | undefined => {
    for (const n of nodes) {
      const hit = n.id === id ? n : find(n.children);
      if (hit) return hit;
    }
    return undefined;
  };
  const category = find(await loadCategories());
  return category ? [TAGS.category(category.handle)] : [TAGS.categories];
}

/** The definition changes a storefront can see: who may read the field, its name and
 * description, its choices' labels, whether it's a facet, and its place. */
const SHOWN_DEFINITION_FIELDS = new Set([
  "name",
  "description",
  "access",
  "validations",
  "capabilities",
  "position",
  "private",
  "personal",
]);

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
    case "product.deleted": {
      // Gone: its page by the handle the event carries (it can't be looked up any more).
      const handle = typeof data.handle === "string" ? data.handle : undefined;
      return { tags: [TAGS.listings, handle ? TAGS.product(handle) : TAGS.products], paths: [] };
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
    // A list started or stopped pricing (its window opened or closed, it joined or left a
    // catalog): any price may have changed. A list on no catalog prices nothing.
    case "price_list.started":
    case "price_list.ended":
      return ids(data.catalogs).length > 0
        ? { tags: [TAGS.prices], paths: [] }
        : { tags: [], paths: [] };
    case "catalog.price_lists_added":
    case "catalog.price_lists_removed":
      return { tags: [TAGS.prices], paths: [] };
    // Custom fields: values appear on product pages and, when a field is a facet, in
    // listings.
    case "product.custom_fields_updated": {
      const product = typeof data.product === "string" ? data.product : event.subject.id;
      return { tags: [TAGS.listings, ...(await productTags([product]))], paths: [] };
    }
    case "category.custom_fields_updated": {
      const category = typeof data.category === "string" ? data.category : event.subject.id;
      return { tags: await categoryTags(category), paths: [] };
    }
    // A definition changed: its values may now be shown, hidden or labelled differently
    // on every page of its owner type. A new one has no values yet.
    case "custom_field_definition.updated":
    case "custom_field_definition.deleted": {
      const fields = ids(data.fields);
      if (
        event.type === "custom_field_definition.updated" &&
        !fields.some((f) => SHOWN_DEFINITION_FIELDS.has(f))
      ) {
        return { tags: [], paths: [] };
      }
      // The definitions themselves too: pages name and order values by them.
      switch (data.owner_type) {
        case "product":
        case "variant":
          return { tags: [TAGS.fields, TAGS.products, TAGS.listings], paths: [] };
        case "category":
          return { tags: [TAGS.fields, TAGS.categories], paths: [] };
        default:
          return { tags: [], paths: [] };
      }
    }
    case "category.changed":
    case "category.deleted":
      return {
        tags: [TAGS.categories, TAGS.listings, ...(await productTags(ids(data.products)))],
        paths: [],
      };
    // An automatic discount's promotion badges show on product pages and in listings, and
    // any product may be one it reaches (a category or variant list, or all of them).
    case "discount.created":
    case "discount.updated":
    case "discount.deleted":
      return { tags: [TAGS.listings, TAGS.products], paths: [] };
    default:
      return { tags: [], paths: [] };
  }
}

export async function POST(request: NextRequest) {
  const secret = process.env.HYPHA_WEBHOOK_SECRET;
  if (!secret) {
    console.error("HYPHA_WEBHOOK_SECRET is not set: static pages won't follow changes");
    return NextResponse.json(
      { ok: false, error: "HYPHA_WEBHOOK_SECRET is not set" },
      { status: 500 },
    );
  }
  const body = await request.text();
  if (!(await verifySignature(secret, request.headers.get("hypha-signature"), body))) {
    return NextResponse.json({ ok: false, error: "invalid signature" }, { status: 401 });
  }
  let event: Envelope;
  try {
    event = JSON.parse(body);
  } catch {
    return NextResponse.json({ ok: false, error: "invalid payload" }, { status: 400 });
  }
  const acted = await deliveries.once(request.headers.get("hypha-delivery-id"), async () => {
    const { tags, paths } = await affected(event);
    // `{ expire: 0 }`: the next visit renders fresh, rather than once more from the stale
    // copy (what "max" would do), which would read as "the change didn't show".
    for (const tag of new Set(tags)) revalidateTag(tag, { expire: 0 });
    for (const path of paths) revalidatePath(path);
    return { tags: [...new Set(tags)], paths };
  });
  if (!acted) return NextResponse.json({ ok: true, repeated: true });
  return NextResponse.json({ ok: true, type: event.type, ...acted });
}
