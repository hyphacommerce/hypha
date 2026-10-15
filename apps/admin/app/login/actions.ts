"use server";

import { redirect } from "next/navigation";

import {
  describe,
  endSession,
  heldChallenge,
  holdChallenge,
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
    redirect(await takeNext());
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
  redirect(await takeNext());
}

/** Ends the session on the API and forgets it here. */
export async function logout(): Promise<void> {
  const client = await sessionClient();
  // Whatever the API says (the session may already be gone), sign out here.
  await client.POST("/admin/auth/logout");
  await endSession();
  redirect("/login");
}
