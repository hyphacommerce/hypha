// What the store's products actually have, for Settings → Search to offer as choices
// (design note 0024, section 6): option names with their values, vendors, product types
// and tags, with how many of the scanned products use each. Read from the newest
// products (summaries for vendors and types, whole products for options and tags), so
// a large catalog costs a few requests, not one per product.

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
    return [...this.counts.values()].sort(
      (a, b) => b.count - a.count || a.value.localeCompare(b.value),
    );
  }
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
