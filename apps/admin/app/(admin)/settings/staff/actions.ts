"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/**
 * Emails an invite to join as `role` (design note 0008, section 4). Inviting an email
 * with a pending invite renews it: a new link, the old one dead.
 */
export async function inviteStaff(email: string, role: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/invites", { body: { email, role } });
  if (error) return { error: describe(error) };
  revalidatePath("/settings/staff");
  return {};
}
