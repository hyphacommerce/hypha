// How the admin shows purchasing: suppliers, their terms and their catalogues, and
// purchase orders.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import type { TFor } from "./messages";

export type Supplier = AdminSchemas["SupplierResponse"];
export type PaymentTerms = AdminSchemas["PaymentTermsDto"];
export type CatalogueRow = AdminSchemas["SupplierVariantResponse"];
export type PurchaseOrder = AdminSchemas["PurchaseOrderResponse"];
export type PurchaseOrderSummary = AdminSchemas["PurchaseOrderSummaryResponse"];
export type PurchaseOrderLine = AdminSchemas["PurchaseOrderLineResponse"];
export type PurchaseOrderCost = AdminSchemas["PurchaseOrderCostResponse"];

/** "PO-1001". */
export function poNumber(number: number): string {
  return `PO-${number}`;
}

/** "Oyster grow kit · Large": what a purchase order line is for. */
export function lineLabel(line: PurchaseOrderLine): string {
  return line.variant_title ? `${line.product_title} · ${line.variant_title}` : line.product_title;
}

/** Whether a purchase order still changes: a draft or an ordered one. */
export function isOpen(po: PurchaseOrder): boolean {
  return po.status === "draft" || po.status === "ordered";
}

/** The kinds of extra cost, in the order offered. */
export const COST_KINDS = [
  "shipping",
  "duties",
  "fees",
  "other",
] as const satisfies readonly AdminSchemas["CostKindDto"][];

/** "Shipping", "Duties"…: an extra cost's kind (an unknown one as it is). */
export function costKindLabel(t: TFor<"purchasing">, kind: string): string {
  const known = COST_KINDS.find((k) => k === kind);
  return known ? t(`purchasing.cost_kind_${known}`) : kind;
}

/** How an extra cost is spread over the lines, in the order offered. */
export const ALLOCATIONS = [
  "by_value",
  "by_quantity",
  "none",
] as const satisfies readonly AdminSchemas["CostAllocationDto"][];

/** "By value", "By quantity"…: how an extra cost is spread (an unknown way as it is). */
export function allocationLabel(t: TFor<"purchasing">, allocation: string): string {
  const known = ALLOCATIONS.find((a) => a === allocation);
  return known ? t(`purchasing.allocation_${known}`) : allocation;
}

/**
 * `amount` minor units of one currency in another at `rate` (one unit of the first in the
 * second, a decimal string with up to 8 places), rounded half away from zero: on
 * integers, so no float creeps in. For showing a total in the store's currency.
 */
export function convertMinor(
  amount: number,
  rate: string,
  fromDigits: number,
  toDigits: number,
): number {
  const [whole = "0", fraction = ""] = rate.split(".");
  const scaledRate = BigInt(whole + fraction.padEnd(8, "0"));
  const numerator = BigInt(amount) * scaledRate * 10n ** BigInt(toDigits);
  const denominator = 10n ** 8n * 10n ** BigInt(fromDigits);
  const quotient = numerator / denominator;
  const remainder = numerator % denominator;
  return Number(remainder * 2n >= denominator ? quotient + 1n : quotient);
}

/** "Net 30", "In advance", "On receipt"; "—" for none. */
export function termsLabel(t: TFor<"purchasing">, terms: PaymentTerms | null | undefined): string {
  if (!terms) return "—";
  switch (terms.type) {
    case "net":
      return t("purchasing.terms_net", { days: terms.days });
    case "in_advance":
      return t("purchasing.terms_in_advance");
    case "on_receipt":
      return t("purchasing.terms_on_receipt");
  }
}

/** "14 days", "1 day"; "—" for none. */
export function daysLabel(t: TFor<"purchasing">, days: number | null | undefined): string {
  if (days === null || days === undefined) return "—";
  return t("purchasing.days_count", { count: days });
}

/** The contact purchase orders go to. */
export function primaryContact(supplier: Supplier) {
  return supplier.contacts.find((c) => c.primary) ?? null;
}

/** "Mug · M / Blue": what a catalogue row is for, as far as the caller may read it. */
export function variantLabel(row: CatalogueRow): string {
  if (!row.product_title) return row.variant;
  return row.variant_title ? `${row.product_title} · ${row.variant_title}` : row.product_title;
}
