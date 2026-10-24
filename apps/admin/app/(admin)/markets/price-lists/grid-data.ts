// Server-only: a price list's prices as the grid and the CSV files work on them. The
// variants come from `GET /admin/variants`, a page at a time, grouped by product; the
// list's adjusted price is worked out here from the list's adjustment (`adjustedPrice`),
// which saves a pricing read per product. Never import this from a "use client" file.

import type { AdminSchemas, ApiError } from "@hyphacommerce/sdk/admin";

import type { CategoryNode } from "@/lib/categories";
import { sessionClient } from "@/lib/hypha";
import {
  type FixedItem,
  type VariantItem,
  groupVariants,
  productIdsOf,
} from "@/lib/price-grid/group";
import { chunk } from "@/lib/price-csv";
import type { PriceGridProduct } from "@/lib/price-grid/types";
import { type PriceList, coveredCategories } from "@/lib/price-lists";

type Client = Awaited<ReturnType<typeof sessionClient>>;
type ProductSummary = AdminSchemas["ProductSummaryResponse"];

/** What the prices tab's URL narrows the grid to. */
export type GridFilters = {
  q?: string;
  /** Products in this category or one below it. */
  category?: string;
  /** Only variants with a fixed price in the list. */
  fixedOnly: boolean;
};

/**
 * Variants per page of the grid. A page may end inside a product: the next page shows
 * the product's header again, above the rest of its variants.
 */
export const GRID_PAGE = 100;
/**
 * The most IDs `GET /admin/products?ids=`, `GET /admin/variants?ids=` and
 * `GET /admin/price-lists/{id}/fixed-prices?variants=` take.
 */
const IDS_PER_READ = 250;
/** The most items a page of any list holds. */
const MAX_LIMIT = 100;
/** Reads at once: enough to be quick, few enough not to flood the API. */
const PARALLEL = 8;

export type GridPage = {
  products: PriceGridProduct[];
  nextCursor: string | null;
  /** The API refused the page: its cursor (`400`), or a filter (`422`). */
  failure?: { status: number; error?: ApiError };
};

/** `run` over `items`, `PARALLEL` at a time, in order. */
async function each<T, R>(items: readonly T[], run: (item: T) => Promise<R>): Promise<R[]> {
  const results: R[] = [];
  for (let i = 0; i < items.length; i += PARALLEL) {
    results.push(...(await Promise.all(items.slice(i, i + PARALLEL).map(run))));
  }
  return results;
}

/** Every page of a list, from the first. */
async function everyPage<T>(
  read: (cursor: string | undefined) => Promise<{ items: T[]; next_cursor?: string | null }>,
): Promise<T[]> {
  const found: T[] = [];
  let cursor: string | undefined;
  do {
    const page = await read(cursor);
    found.push(...page.items);
    cursor = page.next_cursor ?? undefined;
  } while (cursor);
  return found;
}

/** Every fixed price of the list matching `query`. */
function allFixed(
  client: Client,
  list: string,
  query: { variants?: string[]; category?: string; q?: string },
): Promise<FixedItem[]> {
  return everyPage(async (cursor) => {
    const { data, error } = await client.GET("/admin/price-lists/{id}/fixed-prices", {
      params: { path: { id: list }, query: { ...query, cursor, limit: MAX_LIMIT } },
    });
    if (!data) throw new Error(`fixed prices of ${list} failed: ${error?.code ?? "unknown"}`);
    return data;
  });
}

/** Every variant matching `query`, newest product first. */
function allVariants(
  client: Client,
  query: { q?: string; category?: string; ids?: string[] },
): Promise<VariantItem[]> {
  return everyPage(async (cursor) => {
    const { data, error } = await client.GET("/admin/variants", {
      params: { query: { ...query, cursor, limit: MAX_LIMIT } },
    });
    if (!data) throw new Error(`variants failed: ${error?.code ?? "unknown"}`);
    return data;
  });
}

/** These variants (those that still exist), in the API's order, 250 IDs per read. */
async function variantsByIds(client: Client, ids: readonly string[]): Promise<VariantItem[]> {
  const runs = await each(chunk(ids, IDS_PER_READ), (run) => allVariants(client, { ids: run }));
  return runs.flat();
}

/** These products' summaries (those that still exist), 250 IDs per read. */
export async function productSummaries(
  client: Client,
  ids: readonly string[],
): Promise<ProductSummary[]> {
  const runs = await each(chunk([...new Set(ids)], IDS_PER_READ), (run) =>
    everyPage(async (cursor) => {
      const { data, error } = await client.GET("/admin/products", {
        params: { query: { ids: run, cursor, limit: MAX_LIMIT } },
      });
      if (!data) throw new Error(`products failed: ${error?.code ?? "unknown"}`);
      return data;
    }),
  );
  return runs.flat();
}

/**
 * The categories of `items`' products, which only an adjustment by category needs (the
 * variants list doesn't carry them): none read otherwise.
 */
async function categoriesOf(
  client: Client,
  list: PriceList,
  items: readonly VariantItem[],
): Promise<Map<string, readonly string[]>> {
  if (list.adjustment?.applies_to?.type !== "categories" || items.length === 0) return new Map();
  const products = await productSummaries(client, productIdsOf(items));
  return new Map(products.map((p) => [p.id, p.categories]));
}

/**
 * The list's fixed prices for `items`, by variant: read by their IDs, 250 per read, so a
 * grid page of 100 variants is one read whatever the list's size.
 */
async function fixedOf(
  client: Client,
  list: PriceList,
  items: readonly VariantItem[],
): Promise<Map<string, FixedItem>> {
  if (list.fixed_price_count === 0 || items.length === 0) return new Map();
  const ids = [...new Set(items.map((i) => i.id))];
  const runs = await each(chunk(ids, IDS_PER_READ), (variants) =>
    allFixed(client, list.id, { variants }),
  );
  return new Map(runs.flat().map((f) => [f.variant, f]));
}

async function grouped(
  client: Client,
  list: PriceList,
  tree: CategoryNode[],
  items: readonly VariantItem[],
  fixed: Map<string, FixedItem> | null,
): Promise<PriceGridProduct[]> {
  const [prices, categories] = await Promise.all([
    fixed ?? fixedOf(client, list, items),
    categoriesOf(client, list, items),
  ]);
  const covered = coveredCategories(list.adjustment?.applies_to, tree);
  return groupVariants(list, items, prices, categories, covered);
}

/**
 * A page of the grid. Normally a page of variants (newest product first, narrowed by `q`
 * and `category`), grouped by product. With `fixedOnly`, a page of the list's fixed
 * prices instead, grouped by product, each product showing only those variants.
 */
export async function gridPage(
  client: Client,
  list: PriceList,
  tree: CategoryNode[],
  filters: GridFilters,
  cursor: string | undefined,
): Promise<GridPage> {
  if (filters.fixedOnly) {
    const result = await client.GET("/admin/price-lists/{id}/fixed-prices", {
      params: {
        path: { id: list.id },
        query: { q: filters.q, category: filters.category, cursor, limit: MAX_LIMIT },
      },
    });
    if (!result.data) {
      return {
        products: [],
        nextCursor: null,
        failure: { status: result.response.status, error: result.error },
      };
    }
    const fixed = new Map(result.data.items.map((f) => [f.variant, f]));
    // A fixed price whose variant is gone isn't listed: nothing to show it beside.
    const items = await variantsByIds(client, [...fixed.keys()]);
    return {
      products: await grouped(client, list, tree, items, fixed),
      nextCursor: result.data.next_cursor ?? null,
    };
  }

  const result = await client.GET("/admin/variants", {
    params: {
      query: { q: filters.q, category: filters.category, cursor, limit: GRID_PAGE },
    },
  });
  if (!result.data) {
    return {
      products: [],
      nextCursor: null,
      failure: { status: result.response.status, error: result.error },
    };
  }
  return {
    products: await grouped(client, list, tree, result.data.items, null),
    nextCursor: result.data.next_cursor ?? null,
  };
}

/**
 * These variants (those that still exist) with the list's prices, grouped by product: what
 * an import naming every row's variant ID is matched against, without reading the rest
 * of the catalog. A product shows only the variants asked for.
 */
export async function gridProductsByIds(
  client: Client,
  list: PriceList,
  tree: CategoryNode[],
  ids: readonly string[],
): Promise<PriceGridProduct[]> {
  const items = await variantsByIds(client, [...new Set(ids)]);
  return grouped(client, list, tree, items, null);
}

/**
 * Every product matching `filters` (all of them without), with every variant and the
 * list's prices (only those with a fixed price with `fixedOnly`): what an export writes
 * and an import is matched against. Each product is one, its variants together.
 */
export async function allGridProducts(
  client: Client,
  list: PriceList,
  tree: CategoryNode[],
  filters: GridFilters,
): Promise<PriceGridProduct[]> {
  if (filters.fixedOnly) {
    const fixed = await allFixed(client, list.id, { q: filters.q, category: filters.category });
    const byVariant = new Map(fixed.map((f) => [f.variant, f]));
    const items = await variantsByIds(client, [...byVariant.keys()]);
    return grouped(client, list, tree, items, byVariant);
  }
  const [items, fixed] = await Promise.all([
    allVariants(client, { q: filters.q, category: filters.category }),
    list.fixed_price_count > 0 ? allFixed(client, list.id, {}) : Promise.resolve([]),
  ]);
  return grouped(client, list, tree, items, new Map(fixed.map((f) => [f.variant, f])));
}
