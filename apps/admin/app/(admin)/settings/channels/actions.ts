"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };
/** A result with a new publishable key's token. */
export type KeyResult = { error?: string; token?: string };

export type ChannelKind = "online_store" | "agentic" | "custom";

export async function createChannel(name: string, kind: ChannelKind): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/channels", { body: { name, kind } });
  if (error) return { error: describe(error) };
  // Product pages offer every channel: refresh them too.
  revalidatePath("/", "layout");
  return {};
}

export async function renameChannel(id: string, name: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/channels/{id}", {
    params: { path: { id } },
    body: { name },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/", "layout");
  return {};
}

/** A publishable key for a storefront that sells through `channel`. */
export async function createStorefrontKey(channel: string, name: string): Promise<KeyResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/api-keys", {
    body: { kind: "publishable", name, channel },
  });
  if (error) return { error: describe(error) };
  return { token: data.token };
}
