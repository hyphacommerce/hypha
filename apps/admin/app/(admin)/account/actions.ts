"use server";

// The signed-in staff member's own account (the "me" routes). Each answers
// `{error}` for the form to show.

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type Result = { error?: string };
export type TotpSetup = Result & { secret?: string; uri?: string };
export type RecoveryCodes = Result & { codes?: string[] };

const PAGE = "/account";

/** Renames the staff member: their own name, which needs no scope. */
export async function updateName(name: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/me", { body: { name } });
  if (error) return { error: describe(error) };
  revalidatePath("/", "layout");
  return {};
}

/** Signs one of the staff member's other sessions out: that device must sign in again. */
export async function revokeSession(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/me/sessions/{id}", {
    params: { path: { id } },
  });
  // Already signed out (it expired, or another tab did it): the list is just stale.
  if (error && error.code !== "session_not_found") return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

/** Signs out every session of the staff member's but this one. */
export async function revokeOtherSessions(): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/me/sessions/revoke-others");
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

/** Changes the password; every other session of the account ends. */
export async function changePassword(input: {
  current_password: string;
  new_password: string;
}): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/me/password", { body: input });
  if (error) return { error: describe(error) };
  return {};
}

/** Starts setting up an authenticator: the secret, as text and as an `otpauth://` URI. */
export async function startTotp(password: string): Promise<TotpSetup> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/me/mfa/totp", { body: { password } });
  if (error) return { error: describe(error) };
  return { secret: data.secret, uri: data.uri };
}

/** Turns two-factor on with a first code; the recovery codes are shown only now. */
export async function confirmTotp(code: string): Promise<RecoveryCodes> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/me/mfa/totp/confirm", { body: { code } });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return { codes: data.recovery_codes };
}

/** Replaces the recovery codes; the old ones stop working. */
export async function regenerateRecoveryCodes(code: string): Promise<RecoveryCodes> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/me/mfa/recovery-codes", { body: { code } });
  if (error) return { error: describe(error) };
  return { codes: data.recovery_codes };
}

/** Turns two-factor off: the authenticator and the recovery codes both go. */
export async function disableTotp(code: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/me/mfa/totp/disable", { body: { code } });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}
