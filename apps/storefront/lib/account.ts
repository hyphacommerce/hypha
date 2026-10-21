// Server-only: the shopper's account session (design note 0034). The session token
// lives in an httpOnly cookie, like the cart's ID, and rides beside the publishable key
// on the API calls the account pages and actions make. Never import this from a
// "use client" file.

import { cookies } from "next/headers";

import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { api } from "./api";

export type Account = StorefrontSchemas["AccountResponse"];

const SESSION_COOKIE = "customer_session";

/** The session token, if the shopper signed in on this browser. */
export async function sessionToken(): Promise<string | undefined> {
  return (await cookies()).get(SESSION_COOKIE)?.value;
}

/** Keeps the session for its life (the API extends it while it's used). */
export async function rememberSession(token: string) {
  (await cookies()).set(SESSION_COOKIE, token, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  });
}

export async function forgetSession() {
  (await cookies()).delete(SESSION_COOKIE);
}

/** The headers that sign a call as the shopper, none without a session. */
export async function sessionHeaders(): Promise<Record<string, string>> {
  const token = await sessionToken();
  return token ? { authorization: `Bearer ${token}` } : {};
}

/** A client calling as the signed-in shopper; `null` without a session. */
export async function accountApi() {
  const token = await sessionToken();
  return token ? api({ authorization: `Bearer ${token}` }) : null;
}

/**
 * The signed-in shopper's account, or `null` when there's no session or it ended (signed
 * out elsewhere, expired, the customer erased): the cookie is dropped then, so the pages
 * stop asking.
 */
export async function loadAccount(): Promise<Account | null> {
  const client = await accountApi();
  if (!client) return null;
  const { data, response } = await client.GET("/storefront/account");
  if (data) return data;
  if (response.status === 401) {
    await forgetSession();
    return null;
  }
  throw new Error(`/storefront/account failed with ${response.status}`);
}
