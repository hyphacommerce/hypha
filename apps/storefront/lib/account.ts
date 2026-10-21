// Server-only: the shopper's account session (design note 0034). The session token
// lives in an httpOnly cookie, like the cart's ID, and rides beside the publishable key
// on the API calls the account pages and actions make. Never import this from a
// "use client" file.

import { cookies } from "next/headers";

import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { api } from "./api";

export type Account = StorefrontSchemas["AccountResponse"];

export const SESSION_COOKIE = "customer_session";
/** How long the session lasts unused, in seconds (the login's answer), beside the token:
 * `proxy.ts` sets both cookies again on every request, so the cookie slides with use
 * like the session does. Not secret. */
export const SESSION_LIFETIME_COOKIE = "customer_session_ttl";

/** The session token, if the shopper signed in on this browser. */
export async function sessionToken(): Promise<string | undefined> {
  return (await cookies()).get(SESSION_COOKIE)?.value;
}

/** The cookies' options; `maxAge` in seconds. */
export function sessionCookie(maxAge: number) {
  return {
    sameSite: "lax" as const,
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge,
  };
}

/**
 * The session's lifetime in seconds from the login's `expires_at`: the configured
 * `customers.session_lifetime`, never less than a minute (a clock ahead of the API's).
 */
export function sessionLifetime(expiresAt: string): number {
  const seconds = Math.round((Date.parse(expiresAt) - Date.now()) / 1000);
  return Number.isFinite(seconds) ? Math.max(seconds, 60) : 60 * 60 * 24 * 30;
}

/** Keeps the session for its life (the API extends it while it's used). */
export async function rememberSession(token: string, expiresAt: string) {
  const lifetime = sessionLifetime(expiresAt);
  const jar = await cookies();
  jar.set(SESSION_COOKIE, token, { ...sessionCookie(lifetime), httpOnly: true });
  jar.set(SESSION_LIFETIME_COOKIE, String(lifetime), sessionCookie(lifetime));
}

export async function forgetSession() {
  const jar = await cookies();
  jar.delete(SESSION_COOKIE);
  jar.delete(SESSION_LIFETIME_COOKIE);
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
