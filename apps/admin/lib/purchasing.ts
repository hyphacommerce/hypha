// How the admin shows purchasing: suppliers, their terms and their catalogues.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

export type Supplier = AdminSchemas["SupplierResponse"];
export type PaymentTerms = AdminSchemas["PaymentTermsDto"];
export type CatalogueRow = AdminSchemas["SupplierVariantResponse"];

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
