// The grid's rows: each product, then its variants beneath unless it's collapsed. A
// product without options is one row: its only variant, under the product's title.

import type { PriceGridProduct, PriceGridVariant } from "./types.ts";

export type PriceGridRow =
  | { kind: "product"; product: PriceGridProduct; isCollapsed: boolean }
  | {
      kind: "variant";
      product: PriceGridProduct;
      variant: PriceGridVariant;
      /** The product's only row (it has no options). */
      isSingle: boolean;
    };

export function isSingleRow(product: PriceGridProduct): boolean {
  const [only, ...rest] = product.variants;
  return only !== undefined && rest.length === 0 && only.title === null;
}

export function visibleRows(
  products: readonly PriceGridProduct[],
  collapsed: ReadonlySet<string>,
): PriceGridRow[] {
  const rows: PriceGridRow[] = [];
  for (const product of products) {
    if (isSingleRow(product)) {
      const [variant] = product.variants;
      if (variant) rows.push({ kind: "variant", product, variant, isSingle: true });
      continue;
    }
    const isCollapsed = collapsed.has(product.id);
    rows.push({ kind: "product", product, isCollapsed });
    if (isCollapsed) continue;
    for (const variant of product.variants) {
      rows.push({ kind: "variant", product, variant, isSingle: false });
    }
  }
  return rows;
}

/** Every variant, by ID. */
export function variantsById(
  products: readonly PriceGridProduct[],
): ReadonlyMap<string, PriceGridVariant> {
  return new Map(products.flatMap((p) => p.variants.map((v) => [v.variant, v] as const)));
}
