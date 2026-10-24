// Server-only: a price list's prices as the grid and the CSV files work on them. The API
// lists products without their variants, so each product of a page is read whole, with
// its fixed prices in the list; the list's adjusted price is worked out here from the
// list's adjustment (`adjustedPrice`), which saves a pricing read per product. Never
// import this from a "use client" file.

import type { AdminSchemas, ApiError } from "@hyphacommerce/sdk/admin";

import type { CategoryNode } from "@/lib/categories";
import { sessionClient } from "@/lib/hypha";
import type { PriceGridProduct, PriceGridVariant } from "@/lib/price-grid/types";
import { type PriceList, adjustedPrice, coveredCategories } from "@/lib/price-lists";

type Client = Awaited<ReturnType<typeof sessionClient>>;
type Product = AdminSchemas["ProductResponse"];
type Fixed = AdminSchemas["FixedPriceResponse"];

/** What the prices tab's URL narrows the grid to. */
export type GridFilters = {
  q?: string;
  /** Products directly in this category. */
  category?: string;
  /** Only variants with a fixed price in the list. */
  fixedOnly: boolean;
};

/** Products per page of the grid. */
export const GRID_PAGE = 25;
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

async function readProduct(client: Client, id: string): Promise<Product | null> {
  const { data } = await client.GET("/admin/products/{id}", { params: { path: { id } } });
  return data ?? null;
}

/** Every fixed price of the list matching `query`, page after page. */
async function allFixed(
  client: Client,
  list: string,
  query: { product?: string; category?: string; q?: string },
): Promise<Fixed[]> {
  const found: Fixed[] = [];
  let cursor: string | undefined;
  do {
    const { data, error } = await client.GET("/admin/price-lists/{id}/fixed-prices", {
      params: { path: { id: list }, query: { ...query, cursor, limit: 100 } },
    });
    if (!data) throw new Error(`fixed prices of ${list} failed: ${error?.code ?? "unknown"}`);
    found.push(...data.items);
    cursor = data.next_cursor ?? undefined;
  } while (cursor);
  return found;
}

/** The grid's row for `product`: its variants (only `only` when given) with the list's prices. */
function rowOf(
  list: PriceList,
  product: Product,
  fixed: ReadonlyMap<string, Fixed>,
  covered: Set<string> | null,
  only?: ReadonlySet<string>,
): PriceGridProduct {
  const variants: PriceGridVariant[] = product.variants
    .filter((v) => !only || only.has(v.id))
    .map((v) => {
      const own = fixed.get(v.id);
      return {
        variant: v.id,
        title: v.title ?? null,
        sku: v.sku ?? null,
        base_price: v.price,
        compare_at_price: v.compare_at_price ?? null,
        adjusted_price: adjustedPrice(list, product, v.price, covered),
        fixed: own
          ? { price: own.price, compare_at: own.compare_at ?? null, breaks: own.breaks }
          : null,
      };
    });
  return { id: product.id, title: product.title, handle: product.handle, variants };
}

/**
 * A page of the grid. Normally a page of products (newest first, narrowed by `q` and
 * `category`), each with all its variants. With `fixedOnly`, a page of the list's fixed
 * prices instead, grouped by product, each product showing only those variants.
 */
export async function gridPage(
  client: Client,
  list: PriceList,
  tree: CategoryNode[],
  filters: GridFilters,
  cursor: string | undefined,
): Promise<GridPage> {
  const covered = coveredCategories(list.adjustment?.applies_to, tree);
  if (filters.fixedOnly) {
    const result = await client.GET("/admin/price-lists/{id}/fixed-prices", {
      params: {
        path: { id: list.id },
        query: { q: filters.q, category: filters.category, cursor, limit: 100 },
      },
    });
    if (!result.data) {
      return {
        products: [],
        nextCursor: null,
        failure: { status: result.response.status, error: result.error },
      };
    }
    const byProduct = new Map<string, Fixed[]>();
    for (const item of result.data.items) {
      // A fixed price whose variant is gone has no product: nothing to show it beside.
      if (!item.product) continue;
      byProduct.set(item.product.id, [...(byProduct.get(item.product.id) ?? []), item]);
    }
    const products = await each([...byProduct.keys()], (id) => readProduct(client, id));
    return {
      products: products.flatMap((product) => {
        if (!product) return [];
        const items = byProduct.get(product.id) ?? [];
        const fixed = new Map(items.map((f) => [f.variant, f]));
        return [rowOf(list, product, fixed, covered, new Set(fixed.keys()))];
      }),
      nextCursor: result.data.next_cursor ?? null,
    };
  }

  const result = await client.GET("/admin/products", {
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
  const rows = await each(result.data.items, async (summary) => {
    const [product, fixed] = await Promise.all([
      readProduct(client, summary.id),
      list.fixed_price_count > 0
        ? allFixed(client, list.id, { product: summary.id })
        : Promise.resolve([]),
    ]);
    return product
      ? rowOf(list, product, new Map(fixed.map((f) => [f.variant, f])), covered)
      : null;
  });
  return {
    products: rows.filter((row): row is PriceGridProduct => row !== null),
    nextCursor: result.data.next_cursor ?? null,
  };
}

/**
 * Every product matching `filters` (all of them without), with every variant and the
 * list's prices: what an export writes and an import is matched against. Products are
 * read one by one, so a large catalog takes a while.
 */
export async function allGridProducts(
  client: Client,
  list: PriceList,
  tree: CategoryNode[],
  filters: GridFilters,
): Promise<PriceGridProduct[]> {
  if (filters.fixedOnly) {
    const products: PriceGridProduct[] = [];
    let cursor: string | undefined;
    do {
      const page = await gridPage(client, list, tree, filters, cursor);
      if (page.failure)
        throw new Error(`fixed prices of ${list.id} failed: ${page.failure.status}`);
      products.push(...page.products);
      cursor = page.nextCursor ?? undefined;
    } while (cursor);
    // A product whose variants came on several pages: one row, its variants together.
    const merged = new Map<string, PriceGridProduct>();
    for (const p of products) {
      const seen = merged.get(p.id);
      merged.set(p.id, seen ? { ...seen, variants: [...seen.variants, ...p.variants] } : p);
    }
    return [...merged.values()];
  }

  const covered = coveredCategories(list.adjustment?.applies_to, tree);
  const [summaries, fixed] = await Promise.all([
    (async () => {
      const ids: string[] = [];
      let cursor: string | undefined;
      do {
        const { data, error } = await client.GET("/admin/products", {
          params: {
            query: { q: filters.q, category: filters.category, cursor, limit: 100 },
          },
        });
        if (!data) throw new Error(`products failed: ${error?.code ?? "unknown"}`);
        ids.push(...data.items.map((p) => p.id));
        cursor = data.next_cursor ?? undefined;
      } while (cursor);
      return ids;
    })(),
    list.fixed_price_count > 0 ? allFixed(client, list.id, {}) : Promise.resolve([]),
  ]);
  const byVariant = new Map(fixed.map((f) => [f.variant, f]));
  const products = await each(summaries, (id) => readProduct(client, id));
  return products.flatMap((product) => (product ? [rowOf(list, product, byVariant, covered)] : []));
}
