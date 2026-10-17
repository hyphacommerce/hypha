"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** A new key's token, shown once; or why it wasn't made. */
export type CreateResult = { error?: string; token?: string };
export type Result = { error?: string };

/**
 * A secret key with `scopes` (only scopes the caller holds), or a publishable key for a
 * storefront selling through `channel` (design note 0008, 0020).
 */
export async function createApiKey(
  input:
    | { kind: "secret"; name: string; scopes: string[] }
    | { kind: "publishable"; name: string; channel: string },
): Promise<CreateResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/api-keys", {
    body:
      input.kind === "secret"
        ? { kind: "secret", name: input.name, scopes: input.scopes }
        : { kind: "publishable", name: input.name, channel: input.channel },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/settings/api-keys");
  return { token: data.token };
}

/** Revokes a key: calls with it fail from now on. It can't be undone. */
export async function revokeApiKey(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/api-keys/{id}/revoke", { params: { path: { id } } });
  if (error) return { error: describe(error) };
  revalidatePath("/settings/api-keys");
  return {};
}
