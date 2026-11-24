"use server";

// What a shopper does with their account: asking for a code, signing in and out, their
// details, address book, marketing choices and privacy. The session stays in an
// httpOnly cookie (`lib/account.ts`); every change answers with the page re-rendered,
// or the shopper's message when the API refused.

import { refresh } from "next/cache";
import { redirect } from "next/navigation";

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { type ActionResult } from "@/app/[country]/actions";
import { accountApi, forgetSession, rememberSession, sessionToken } from "@/lib/account";
import { api, pageLocale, refusedWith, shopperApi } from "@/lib/api";
import { actionT } from "@/lib/i18n";
import { cartId, link } from "@/lib/site";

export type AddressInput = StorefrontSchemas["AddressRequest"];

async function refused(
  locale: string,
  error: { code: string; error: string; details?: unknown },
): Promise<ActionResult> {
  return refusedWith(await actionT(locale), error);
}

/** A link in the page's place: `locale` is its locale segment, as the form sent it,
 * read again here so nothing but a place's own segment reaches the URL. */
async function to(locale: string, path: string): Promise<string> {
  return link((await pageLocale(locale)).segment, path);
}

/** `value` trimmed, or `null` for a blank optional field. */
function optional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

/** Back to the sign-in page when the session ended meanwhile. */
async function signedIn(locale: string) {
  const client = await accountApi();
  if (!client) redirect(await to(locale, "/account"));
  return client;
}

/** Emails a sign-in code; the same answer whatever the address (the API tells nobody who
 * is a customer). */
export async function requestCode(locale: string, email: string): Promise<ActionResult> {
  // The page's language, so the code's email is in it (and a new customer keeps it).
  const { language } = await pageLocale(locale);
  const { error } = await (
    await shopperApi()
  ).POST("/storefront/auth/code", { body: { email: email.trim(), language } });
  if (error) return refused(locale, error);
  return undefined;
}

/** Signs in with the emailed code: the session goes in the cookie, the cart becomes the
 * customer's, and the shopper lands on their account. */
export async function signIn(locale: string, email: string, code: string): Promise<ActionResult> {
  const cart = await cartId((await pageLocale(locale)).country);
  const client = await shopperApi(cart ? { "x-hypha-cart": cart } : {});
  const { data, error } = await client.POST("/storefront/auth/login", {
    body: { email: email.trim(), code: code.replace(/\s+/g, "") },
  });
  if (error) return refused(locale, error);
  await rememberSession(data.token, data.expires_at);
  redirect(await to(locale, "/account"));
}

export async function signOut(locale: string) {
  const token = await sessionToken();
  if (token) {
    await api({ authorization: `Bearer ${token}` }).POST("/storefront/account/logout");
    await forgetSession();
  }
  redirect(await to(locale, "/"));
}

/** Ends every session of the account, on every device. */
export async function signOutEverywhere(locale: string) {
  const token = await sessionToken();
  if (token) {
    await api({ authorization: `Bearer ${token}` }).POST("/storefront/account/logout-all");
    await forgetSession();
  }
  redirect(await to(locale, "/"));
}

export async function updateProfile(
  locale: string,
  input: { first_name: string; last_name: string; phone: string; language?: string },
): Promise<ActionResult> {
  const client = await signedIn(locale);
  const { error } = await client.PATCH("/storefront/account", {
    body: {
      first_name: optional(input.first_name),
      last_name: optional(input.last_name),
      phone: optional(input.phone),
      // Only when picked: absent keeps the one they have (perhaps from another country,
      // which this page doesn't list). The API holds it to the store's languages.
      ...(input.language ? { language: input.language } : {}),
    },
  });
  if (error) return refused(locale, error);
  refresh();
  return undefined;
}

/** Adds an address, or replaces `address` whole (defaults included). */
export async function saveAddress(
  locale: string,
  address: string | null,
  body: AddressInput,
): Promise<ActionResult> {
  const client = await signedIn(locale);
  const { error } = address
    ? await client.PUT("/storefront/account/addresses/{address_id}", {
        params: { path: { address_id: address } },
        body,
      })
    : await client.POST("/storefront/account/addresses", { body });
  if (error) return refused(locale, error);
  refresh();
  return undefined;
}

export async function removeAddress(locale: string, address: string): Promise<ActionResult> {
  const client = await signedIn(locale);
  const { error } = await client.DELETE("/storefront/account/addresses/{address_id}", {
    params: { path: { address_id: address } },
  });
  if (error) return refused(locale, error);
  refresh();
  return undefined;
}

/** The shopper's own word on marketing by `channel`, now. */
export async function setMarketing(
  locale: string,
  channel: "email" | "sms",
  subscribed: boolean,
): Promise<ActionResult> {
  const client = await signedIn(locale);
  const { error } = await client.PUT("/storefront/account/marketing/{channel}", {
    params: { path: { channel } },
    body: { state: subscribed ? "subscribed" : "unsubscribed" },
  });
  if (error) return refused(locale, error);
  refresh();
  return undefined;
}

export async function requestErasure(locale: string): Promise<ActionResult> {
  const client = await signedIn(locale);
  const { error } = await client.POST("/storefront/account/erasure");
  if (error) return refused(locale, error);
  refresh();
  return undefined;
}

export async function cancelErasure(locale: string): Promise<ActionResult> {
  const client = await signedIn(locale);
  const { error } = await client.DELETE("/storefront/account/erasure");
  if (error) return refused(locale, error);
  refresh();
  return undefined;
}
