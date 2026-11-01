"use server";

import { sessionClient } from "@/lib/hypha";

/** What a new purchase order is written from: active suppliers and the store's locations. */
export async function purchaseOrderChoices(): Promise<{
  suppliers: { id: string; name: string; currency: string }[];
  locations: { id: string; name: string }[];
}> {
  const client = await sessionClient();
  const [suppliers, locations] = await Promise.all([
    client.GET("/admin/suppliers", { params: { query: { archived: false, limit: 100 } } }),
    client.GET("/admin/locations"),
  ]);
  return {
    suppliers: (suppliers.data?.items ?? []).map((s) => ({
      id: s.id,
      name: s.name,
      currency: s.currency,
    })),
    locations: (locations.data?.items ?? []).map((l) => ({ id: l.id, name: l.name })),
  };
}
