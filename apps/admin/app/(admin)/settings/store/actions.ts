"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type FormState = { error?: string; saved?: boolean };

/** A JSON Merge Patch of the store: absent keeps a field, `null` clears it. */
export type StorePatch = AdminSchemas["UpdateStoreRequest"];

export async function updateStore(_previous: FormState, patch: StorePatch): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/store", { body: patch });
  if (error) return { error: describe(error) };
  // The name shows in the shell, and the setup checklist reads the details.
  revalidatePath("/", "layout");
  return { saved: true };
}
