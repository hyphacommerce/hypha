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
  if (error) return { error: await describe(error) };
  revalidatePath(`/inventory/${item}`);
  return {};
}

/**
 * Sets what one unit cost, in the store's base currency (`amount` in minor units as a
 * decimal string), or clears it with `null`, for a reason the history keeps.
 */
export async function setUnitCost(
  item: string,
  input: { cost: { amount: string; currency: string } | null; note: string },
): Promise<{ error?: string }> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/inventory-items/{id}/cost", {
    params: { path: { id: item } },
    body: { unit_cost: input.cost, note: input.note.trim() === "" ? null : input.note },
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/inventory/${item}`);
  return {};
}
