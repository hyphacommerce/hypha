"use server";

import { revalidatePath } from "next/cache";

import type { AdjustmentReason } from "@/lib/inventory";
import { describe, sessionClient } from "@/lib/hypha";

/** Adds or removes units at a location by hand, for a reason the history keeps. */
export async function adjustStock(
  item: string,
  input: { location: string; delta: number; reason: AdjustmentReason; note: string },
): Promise<{ error?: string }> {
  const client = await sessionClient();
  const { error } = await client.POST(
    "/admin/inventory-items/{id}/levels/{location_id}/adjustments",
    {
      params: { path: { id: item, location_id: input.location } },
      body: {
        delta: input.delta,
        reason: input.reason,
        note: input.note.trim() === "" ? null : input.note,
      },
    },
  );
  if (error) return { error: describe(error) };
  revalidatePath(`/inventory/${item}`);
  return {};
}
