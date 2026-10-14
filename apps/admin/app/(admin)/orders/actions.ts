"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type ActionResult = { error?: string };

/** Records that the order was paid outside Hypha (no payment spores yet, 0007 §10). */
export async function markOrderPaid(id: string): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/orders/{id}/paid", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/orders/${id}`);
  revalidatePath("/orders");
  return {};
}
