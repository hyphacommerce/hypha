"use server";

// The signed-in staff member's own account (design note 0008, section 6: the "me"
// routes). Each answers `{error}` for the form to show.

import { revalidatePath } from "next/cache";

import { describe, requireStaff, sessionClient } from "@/lib/hypha";

export type Result = { error?: string };
export type TotpSetup = Result & { secret?: string; uri?: string };
export type RecoveryCodes = Result & { codes?: string[] };

const PAGE = "/account";

/**
 * Renames the staff member. The API has no "me" route for it, so it's the user's own
 * record, which needs `users:write`.
 */
export async function updateName(name: string): Promise<Result> {
  const [client, me] = await Promise.all([sessionClient(), requireStaff()]);
  const { error } = await client.PATCH("/admin/users/{id}", {
    params: { path: { id: me.user.id } },
    body: { name },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/", "layout");
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
