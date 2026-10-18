"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type SearchSettings = AdminSchemas["UpdateSearchSettingsRequest"];
export type SearchStatus = AdminSchemas["SearchStatusResponse"];

export async function saveSearchSettings(settings: SearchSettings): Promise<{ error?: string }> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/search/settings", { body: settings });
  if (error) return { error: describe(error) };
  revalidatePath("/settings/search");
  return {};
}

/** Queues a rebuild of every search document; the status right after. */
export async function rebuildIndex(): Promise<{ error?: string; status?: SearchStatus }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/search/reindex");
  if (error) return { error: describe(error) };
  return { status: data };
}

export async function indexStatus(): Promise<{ error?: string; status?: SearchStatus }> {
  const client = await sessionClient();
  const { data, error } = await client.GET("/admin/search/status");
  if (error) return { error: describe(error) };
  return { status: data };
}
