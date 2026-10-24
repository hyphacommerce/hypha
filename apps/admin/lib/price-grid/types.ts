// What the price grid and the price list CSV files work on. The shapes mirror the API's
// planned JSON (`Money` is the SDK's `{amount, currency}`; a fixed price is `{variant,
// price, compare_at, breaks}`, as `PUT /admin/price-lists/{id}/fixed-prices` takes it),
// so wiring them to the SDK is a mapping, not a rewrite.

/** An amount in integer minor units (1050 is 10.50 EUR), as the SDK's `Money`. */
export type Money = { amount: number; currency: string };

/** "From `min_quantity` on, each costs `price`." */
export type PriceBreak = { min_quantity: number; price: Money };

/** A fixed price as the list holds it. */
export type FixedPrice = {
  price: Money;
  compare_at: Money | null;
  /** Ascending by quantity; empty for none. */
  breaks: PriceBreak[];
};

export type PriceGridVariant = {
  /** `var_…`. */
  variant: string;
  /** The option values ("M / Blue"); null for a product without options. */
  title: string | null;
  sku: string | null;
  /** In the variant's base currency, which may not be the list's. */
  base_price: Money;
  compare_at_price: Money | null;
  /**
   * What the list's adjustment makes of the base price, when the adjustment covers the
   * variant's product; null when it doesn't (then only a fixed price prices it here).
   */
  adjusted_price: Money | null;
  /** The list's fixed price for the variant, if it has one. */
  fixed: FixedPrice | null;
};

export type PriceGridProduct = {
  /** `prod_…`. */
  id: string;
  title: string;
  handle: string;
  variants: PriceGridVariant[];
};

/** One fixed price to write: an entry of `PUT …/fixed-prices`. */
export type FixedPriceUpsert = {
  variant: string;
  price: Money;
  compare_at: Money | null;
  breaks: PriceBreak[];
};

/**
 * What saving changes: fixed prices to write (`PUT …/fixed-prices`, at most 1,000 per
 * request) and variants whose fixed price goes (`DELETE …/fixed-prices`).
 */
export type PriceGridDiff = {
  upserts: FixedPriceUpsert[];
  clears: { variant: string }[];
};

/** The cells a merchant edits. */
export type EditableColumn = "fixed_price" | "fixed_compare_at" | "breaks";

/** Every column the grid can show, besides the product and variant names. */
export type PriceGridColumn =
  | "base_price"
  | "compare_at_price"
  | "list_price"
  | "fixed_price"
  | "fixed_compare_at"
  | "breaks";

export const EDITABLE_COLUMNS: readonly EditableColumn[] = [
  "fixed_price",
  "fixed_compare_at",
  "breaks",
];

export function isEditableColumn(column: PriceGridColumn): column is EditableColumn {
  return (EDITABLE_COLUMNS as readonly string[]).includes(column);
}

/** The most breaks a fixed price may have. */
export const MAX_BREAKS = 10;
