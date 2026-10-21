"use server";

// What a shopper does with their account (design note 0034): asking for a code, signing
// in and out, their details, address book, marketing choices and privacy. The session
// stays in an httpOnly cookie (`lib/account.ts`); every change answers with the page
// re-rendered, or the shopper's message when the API refused.

import { refresh } from "next/cache";
import { redirect } from "next/navigation";

import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { type ActionResult } from "@/app/[country]/actions";
import { accountApi, forgetSession, rememberSession, sessionToken } from "@/lib/account";
import { api, describe, shopperApi } from "@/lib/api";
import { cartId, link } from "@/lib/site";

export type AddressInput = StorefrontSchemas["AddressRequest"];

function refused(error: { code: string; error: string }): ActionResult {
  return { error: describe(error) };
}

/** `value` trimmed, or `null` for a blank optional field. */
function optional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

/** Back to the sign-in page when the session ended meanwhile. */
async function signedIn(country: string) {
  const client = await accountApi();
  if (!client) redirect(link(country, "/account"));
  return client;
}

/** Emails a sign-in code; the same answer whatever the address (the API tells nobody who
 * is a customer). */
export async function requestCode(country: string, email: string): Promise<ActionResult> {
  const { error } = await (
    await shopperApi()
  ).POST("/storefront/auth/code", { body: { email: email.trim() } });
  if (error) return refused(error);
  return undefined;
}

/** Signs in with the emailed code: the session goes in the cookie, the cart becomes the
 * customer's, and the shopper lands on their account. */
export async function signIn(country: string, email: string, code: string): Promise<ActionResult> {
  const cart = await cartId(country);
  const client = await shopperApi(cart ? { "x-hypha-cart": cart } : {});
  const { data, error } = await client.POST("/storefront/auth/login", {
    body: { email: email.trim(), code: code.replace(/\s+/g, "") },
  });
  if (error) return refused(error);
  await rememberSession(data.token, data.expires_at);
  redirect(link(country, "/account"));
}

export async function signOut(country: string) {
  const token = await sessionToken();
  if (token) {
    await api({ authorization: `Bearer ${token}` }).POST("/storefront/account/logout");
    await forgetSession();
  }
  redirect(link(country, "/"));
}

/** Ends every session of the account, on every device. */
export async function signOutEverywhere(country: string) {
  const token = await sessionToken();
  if (token) {
    await api({ authorization: `Bearer ${token}` }).POST("/storefront/account/logout-all");
    await forgetSession();
  }
  redirect(link(country, "/"));
}

export async function updateProfile(
  country: string,
  input: { first_name: string; last_name: string; phone: string },
): Promise<ActionResult> {
  const client = await signedIn(country);
  const { error } = await client.PATCH("/storefront/account", {
    body: {
      first_name: optional(input.first_name),
      last_name: optional(input.last_name),
      phone: optional(input.phone),
    },
  });
  if (error) return refused(error);
  refresh();
  return undefined;
}

/** Adds an address, or replaces `address` whole (defaults included). */
export async function saveAddress(
  country: string,
  address: string | null,
  body: AddressInput,
): Promise<ActionResult> {
  const client = await signedIn(country);
  const { error } = address
    ? await client.PUT("/storefront/account/addresses/{address_id}", {
        params: { path: { address_id: address } },
        body,
      })
    : await client.POST("/storefront/account/addresses", { body });
  if (error) return refused(error);
  refresh();
  return undefined;
}

export async function removeAddress(country: string, address: string): Promise<ActionResult> {
  const client = await signedIn(country);
  const { error } = await client.DELETE("/storefront/account/addresses/{address_id}", {
    params: { path: { address_id: address } },
  });
  if (error) return refused(error);
  refresh();
  return undefined;
}

/** The shopper's own word on marketing by `channel`, now. */
export async function setMarketing(
  country: string,
  channel: "email" | "sms",
  subscribed: boolean,
): Promise<ActionResult> {
  const client = await signedIn(country);
  const { error } = await client.PUT("/storefront/account/marketing/{channel}", {
    params: { path: { channel } },
    body: { state: subscribed ? "subscribed" : "unsubscribed" },
  });
  if (error) return refused(error);
  refresh();
  return undefined;
}

export async function requestErasure(country: string): Promise<ActionResult> {
  const client = await signedIn(country);
  const { error } = await client.POST("/storefront/account/erasure");
  if (error) return refused(error);
  refresh();
  return undefined;
}

export async function cancelErasure(country: string): Promise<ActionResult> {
  const client = await signedIn(country);
  const { error } = await client.DELETE("/storefront/account/erasure");
  if (error) return refused(error);
  refresh();
  return undefined;
}
