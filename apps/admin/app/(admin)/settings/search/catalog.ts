// What the store's products actually have, for Settings → Search to offer as choices:
// option names with their values, vendors, product types and tags, with how many of the
// scanned products use each. Read from the newest products (summaries for vendors and
// types, whole products for options and tags), so a large catalog costs a few requests,
// not one per product.

import type { sessionClient } from "@/lib/hypha";

type Client = Awaited<ReturnType<typeof sessionClient>>;

/** A value and how many scanned products have it. */
export type Counted = { value: string; count: number };

export type SearchCatalog = {
  /** Option names (first spelling seen), most used first, with their values. */
  options: { name: string; count: number; values: Counted[] }[];
  vendors: Counted[];
  productTypes: Counted[];
  tags: Counted[];
  /**
   * Custom fields search can filter by: product and variant fields that are filterable,
   * readable by storefronts, and text, choice, true/false or integer (or a list of
   * those). `key` is the facet's (`field.custom.material`); values are those in use, a
   * choice's by its label.
   */
  fields: { key: string; name: string; count: number; values: Counted[] }[];
  /** How many products the choices come from, and whether the store has more. */
  scanned: number;
  hasMore: boolean;
};

/** Summaries read for vendors and types; whole products read for options and tags. */
const SUMMARIES = 200;
const DETAILS = 60;

/** Counts values ignoring case, keeping the first spelling, most used first. */
class Tally {
  private counts = new Map<string, Counted>();

  add(value: string | null | undefined) {
    const v = value?.trim();
    if (!v) return;
    const key = v.toLowerCase();
    const found = this.counts.get(key);
    if (found) found.count += 1;
    else this.counts.set(key, { value: v, count: 1 });
  }

  sorted(): Counted[] {
    return [...this.counts.values()].toSorted(
      (a, b) => b.count - a.count || a.value.localeCompare(b.value),
    );
  }
}

/** Field types search builds facets from (`hypha_search::facet_field`). */
const FACET_TYPES = new Set(["text", "choice", "boolean", "integer"]);
/** Values in use read per field: enough for the preview's sample. */
const FIELD_VALUES = 50;

async function facetFields(client: Client): Promise<SearchCatalog["fields"]> {
  const { data } = await client.GET("/admin/custom-field-definitions");
  const eligible = (data?.items ?? []).filter(
    (d) =>
      (d.owner_type === "product" || d.owner_type === "variant") &&
      d.capabilities.filterable &&
      d.access.storefront === "read" &&
      FACET_TYPES.has(d.type.replace(/^list\./, "")),
  );
  return Promise.all(
    eligible.map(async (d) => {
      const { data: used } = await client.GET(
        "/admin/custom-field-definitions/{id}/values-in-use",
        {
          params: { path: { id: d.id }, query: { limit: FIELD_VALUES } },
        },
      );
      // What shoppers see: a choice's label, a yes or no (as search labels them).
      const labels = new Map<string, string>(
        d.type === "boolean"
          ? [
              ["true", "Yes"],
              ["false", "No"],
            ]
          : (d.validations.choices ?? []).map((c) => [c.key, c.label]),
      );
      const values = (used?.items ?? []).map(({ value, owners }) => {
        const text = typeof value === "string" ? value : JSON.stringify(value);
        return { value: labels.get(text) ?? text, count: owners };
      });
      return {
        key: `field.${d.field}`,
        name: d.name,
        count: values.reduce((n, v) => n + v.count, 0),
        values,
      };
    }),
  );
}

export async function loadSearchCatalog(client: Client): Promise<SearchCatalog> {
  const ids: string[] = [];
  const vendors = new Tally();
  const productTypes = new Tally();
  let cursor: string | undefined;
  let hasMore = false;
  while (ids.length < SUMMARIES) {
    const { data } = await client.GET("/admin/products", {
      params: { query: { limit: 100, cursor } },
    });
    if (!data) break;
    for (const p of data.items) {
      ids.push(p.id);
      vendors.add(p.vendor);
      productTypes.add(p.product_type);
    }
    hasMore = data.has_more;
    if (!data.next_cursor) break;
    cursor = data.next_cursor;
  }

  const details = await Promise.all(
    ids
      .slice(0, DETAILS)
      .map((id) =>
        client.GET("/admin/products/{id}", { params: { path: { id } } }).then((r) => r.data),
      ),
  );
  const tags = new Tally();
  const optionNames = new Tally();
  const optionValues = new Map<string, Tally>();
  for (const product of details) {
    if (!product) continue;
    for (const tag of product.tags) tags.add(tag);
    for (const option of product.options) {
      optionNames.add(option.name);
      const key = option.name.trim().toLowerCase();
      const values = optionValues.get(key) ?? new Tally();
      for (const v of option.values) values.add(v.label);
      optionValues.set(key, values);
    }
  }

  return {
    fields: await facetFields(client),
    options: optionNames.sorted().map(({ value, count }) => ({
      name: value,
      count,
      values: optionValues.get(value.toLowerCase())?.sorted() ?? [],
    })),
    vendors: vendors.sorted(),
    productTypes: productTypes.sorted(),
    tags: tags.sorted(),
    scanned: ids.length,
    hasMore: hasMore || ids.length > DETAILS,
  };
}
