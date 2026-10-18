"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

const PAGE = "/settings/staff";

/**
 * Emails an invite to join as `role` (design note 0008, section 4). Inviting an email
 * with a pending invite renews it: a new link, the old one dead.
 */
export async function inviteStaff(email: string, role: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/invites", { body: { email, role } });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

/**
 * Moves a staff member to another role, or disables or enables them. Disabling ends
 * their sessions and reset links at once; the last active owner can't be moved or
 * disabled (`last_owner`).
 */
export async function updateStaff(
  id: string,
  change: { role?: string; status?: "active" | "disabled" },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/users/{id}", {
    params: { path: { id } },
    body: change,
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

/** Creates a role, or renames one and sets its scopes. Nobody grants a scope they lack. */
export async function saveRole(
  id: string | undefined,
  body: { name: string; scopes: AdminSchemas["Scope"][] },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = id
    ? await client.PATCH("/admin/roles/{id}", { params: { path: { id } }, body })
    : await client.POST("/admin/roles", { body });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}
