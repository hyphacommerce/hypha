"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type FormState = { error?: string; saved?: boolean };

export async function renameStore(_previous: FormState, name: string): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/store", { body: { name } });
  if (error) return { error: describe(error) };
  revalidatePath("/", "layout");
  return { saved: true };
}
