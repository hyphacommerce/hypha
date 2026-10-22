"use server";

import { redirect } from "next/navigation";

import {
  describe,
  endSession,
  heldChallenge,
  holdChallenge,
  landing,
  publicClient,
  sessionClient,
  startSession,
  takeNext,
} from "@/lib/hypha";

export type FormState = { error?: string };

/** The first login step. A second factor owed sends the user to the code page. */
export async function login(
  _previous: FormState,
  input: { email: string; password: string },
): Promise<FormState> {
  const client = await publicClient();
  const { data, error } = await client.POST("/admin/auth/login", { body: input });
  if (error) return { error: describe(error) };
  if (data.status === "authenticated") {
    await startSession(data.session);
    redirect(await landing(data.session.token, await takeNext()));
  }
  await holdChallenge(data.challenge);
  redirect("/login/mfa");
}

/** The second step: a code from the authenticator app, or a recovery code. */
export async function finishLogin(_previous: FormState, code: string): Promise<FormState> {
  const challenge = await heldChallenge();
  if (!challenge) redirect("/login");
  const client = await publicClient();
  const { data, error } = await client.POST("/admin/auth/login/mfa", {
    body: { challenge, code },
  });
  if (error) return { error: describe(error) };
  if (data.status !== "authenticated") redirect("/login");
  await startSession(data.session);
  redirect(await landing(data.session.token, await takeNext()));
}

/**
 * Asks for a password reset link. The API answers the same whether or not the email has
 * an account, so this says the same too.
 */
export async function requestPasswordReset(
  _previous: FormState & { sentTo?: string },
  email: string,
): Promise<FormState & { sentTo?: string }> {
  const client = await publicClient();
  const { error } = await client.POST("/admin/auth/password-reset", { body: { email } });
  if (error) return { error: describe(error) };
  return { sentTo: email };
}

/** Sets a new password with a reset link's token; every session of the account ends. */
export async function confirmPasswordReset(
  _previous: FormState,
  input: { token: string; password: string },
): Promise<FormState> {
  const client = await publicClient();
  const { error } = await client.POST("/admin/auth/password-reset/confirm", { body: input });
  if (error) {
    return {
      error:
        error.code === "invalid_token"
          ? "This reset link expired or was already used. Ask for a new one."
          : describe(error),
    };
  }
  redirect("/login?reset=done");
}

/**
 * Joins the store with an invite's token: the API makes the user and answers with a
 * session, so accepting signs the new staff member in.
 */
export async function acceptInvite(
  _previous: FormState,
  input: { token: string; name: string; password: string },
): Promise<FormState> {
  const client = await publicClient();
  const { data, error } = await client.POST("/admin/invites/accept", { body: input });
  if (error) {
    return {
      error:
        error.code === "invalid_token"
          ? "This invite expired or was already used. Ask the person who invited you for a new one."
          : describe(error),
    };
  }
  await startSession(data.session);
  redirect(await landing(data.session.token, "/"));
}

/** Ends the session on the API and forgets it here. */
export async function logout(): Promise<void> {
  const client = await sessionClient();
  // Whatever the API says (the session may already be gone), sign out here.
  await client.POST("/admin/auth/logout");
  await endSession();
  redirect("/login");
}
