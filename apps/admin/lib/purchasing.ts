// How the admin shows purchasing: suppliers, their terms and their catalogues, and
// purchase orders.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

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

export const COST_KINDS: { value: AdminSchemas["CostKindDto"]; label: string }[] = [
  { value: "shipping", label: "Shipping" },
  { value: "duties", label: "Duties" },
  { value: "fees", label: "Fees" },
  { value: "other", label: "Other" },
];

export const ALLOCATIONS: {
  value: AdminSchemas["CostAllocationDto"];
  label: string;
  description: string;
}[] = [
  { value: "by_value", label: "By value", description: "Each line's share of the order's value." },
  { value: "by_quantity", label: "By quantity", description: "Each line's share of the units." },
  { value: "none", label: "Not in unit costs", description: "An expense on the total only." },
];

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
export function termsLabel(terms: PaymentTerms | null | undefined): string {
  if (!terms) return "—";
  switch (terms.type) {
    case "net":
      return `Net ${terms.days}`;
    case "in_advance":
      return "In advance";
    case "on_receipt":
      return "On receipt";
  }
}

/** "14 days", "1 day"; "—" for none. */
export function daysLabel(days: number | null | undefined): string {
  if (days === null || days === undefined) return "—";
  return days === 1 ? "1 day" : `${days} days`;
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
