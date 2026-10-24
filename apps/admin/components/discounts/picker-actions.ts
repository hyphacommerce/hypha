"use server";

// What the discount form's variant and customer pickers read: a page of each, searched
// as the API searches them, as rows with a label and a line under it.

import { customerName, formatMoney } from "@/lib/format";
import { loadCurrencies, sessionClient } from "@/lib/hypha";

/** A row a picker shows: what's picked (its ID) and how it reads. */
export type PickerRow = { id: string; label: string; detail: string };

export type PickerRows = { items: PickerRow[]; nextCursor: string | null; error?: string };

/** Variants whose product's title or handle, or own SKU, contains `q`. */
export async function searchVariants(q: string, cursor?: string): Promise<PickerRows> {
  const client = await sessionClient();
  const [{ data, error }, currencies] = await Promise.all([
    client.GET("/admin/variants", {
      params: { query: { q: q.trim() || undefined, cursor, limit: 25 } },
    }),
    loadCurrencies(),
  ]);
  if (!data) {
    return { items: [], nextCursor: null, error: error?.error ?? "Couldn't load variants." };
  }
  return {
    items: data.items.map((v) => ({
      id: v.id,
      label: v.title ? `${v.product.title} – ${v.title}` : v.product.title,
      detail: [v.sku, formatMoney(v.price, currencies)].filter(Boolean).join(" · "),
    })),
    nextCursor: data.next_cursor ?? null,
  };
}

/** Customers whose name or email contains `q`. */
export async function searchCustomers(q: string, cursor?: string): Promise<PickerRows> {
  const client = await sessionClient();
  const { data, error } = await client.GET("/admin/customers", {
    params: { query: { q: q.trim() || undefined, cursor, limit: 25 } },
  });
  if (!data) {
    return { items: [], nextCursor: null, error: error?.error ?? "Couldn't load customers." };
  }
  return {
    items: data.items.map((c) => {
      const name = customerName(c);
      const email = c.email ?? "";
      return { id: c.id, label: name || email || c.id, detail: name ? email : "" };
    }),
    nextCursor: data.next_cursor ?? null,
  };
}
