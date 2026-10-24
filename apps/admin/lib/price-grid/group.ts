// The grid's products from `GET /admin/variants`: its variants grouped by product, each
// with the list's fixed and adjusted prices. Plain functions (run by `node --test`).

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { type Adjustment, adjustedPrice } from "../price-lists.ts";
import type { PriceGridProduct } from "./types.ts";

/** A variant as `GET /admin/variants` lists it. */
export type VariantItem = AdminSchemas["VariantSummaryResponse"];

/** A fixed price as `GET /admin/price-lists/{id}/fixed-prices` lists it. */
export type FixedItem = AdminSchemas["FixedPriceResponse"];

/**
 * `items` grouped by product, in the order their products first appear (the API lists a
 * product's variants together, so the products keep the page's order). `categories`
 * holds each product's categories when the list's adjustment is by category (`covered`);
 * a product missing from it counts as in none.
 */
export function groupVariants(
  list: { currency: string; adjustment?: Adjustment | null },
  items: readonly VariantItem[],
  fixed: ReadonlyMap<string, FixedItem>,
  categories: ReadonlyMap<string, readonly string[]>,
  covered: Set<string> | null,
): PriceGridProduct[] {
  const products = new Map<string, PriceGridProduct>();
  for (const item of items) {
    const { id, title, handle } = item.product;
    let product = products.get(id);
    if (!product) {
      product = { id, title, handle, variants: [] };
      products.set(id, product);
    }
    const own = fixed.get(item.id);
    product.variants.push({
      variant: item.id,
      title: item.title ?? null,
      sku: item.sku ?? null,
      base_price: item.price,
      compare_at_price: item.compare_at_price ?? null,
      adjusted_price: adjustedPrice(
        list,
        { id, categories: categories.get(id) ?? [] },
        item.price,
        covered,
      ),
      fixed: own
        ? { price: own.price, compare_at: own.compare_at ?? null, breaks: own.breaks }
        : null,
    });
  }
  return [...products.values()];
}

/** The distinct products of `items`, in order. */
export function productIdsOf(items: readonly VariantItem[]): string[] {
  return [...new Set(items.map((v) => v.product.id))];
}
