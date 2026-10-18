// Server-only: how the admin's server talks to the Hypha admin API (design note 0011,
// P4). The staff session lives in an httpOnly cookie that page scripts can't read; only
// code here sends it to the API. Never import this from a "use client" file.

import { apiFetch } from "#api-fetch";
import { type AdminSchemas, type ApiError, createAdminClient } from "@hypha/sdk/admin";
import { cookies, headers } from "next/headers";
import { notFound, redirect } from "next/navigation";
import { cache } from "react";

import { sessionFetch } from "./api-cache";
import type { Currencies } from "./format";
import type { Country } from "./regions";

const SESSION_COOKIE = "hypha_admin_session";
/** A pending login's second-factor challenge, for the few minutes it lives. */
const CHALLENGE_COOKIE = "hypha_admin_challenge";
/** Where to go after signing in again, set when a session expired. */
const NEXT_COOKIE = "hypha_admin_next";

function apiUrl(): string {
  const url = process.env.HYPHA_API_URL;
  if (!url) {
    throw new Error("HYPHA_API_URL is not set (see apps/admin/.env.example)");
  }
  return url;
}

/**
 * The user's address, passed on so Hypha's per-address rate limits (design note 0009)
 * count the user, not the admin server. Hypha only believes it when the admin's address
 * is in `HYPHA_TRUSTED_PROXIES`.
 *
 * Only from a source the browser can't forge: Cloudflare's `cf-connecting-ip` (set on
 * Workers), or, when `ADMIN_TRUST_X_FORWARDED_FOR=true` because the admin runs behind
 * its own proxy, the last `x-forwarded-for` entry (the one that proxy appended; earlier
 * entries are whatever the client sent). Otherwise nothing, and the limits count the
 * admin server's address: stricter, never bypassable.
 */
async function forwardedFor(): Promise<Record<string, string>> {
  const h = await headers();
  let ip = h.get("cf-connecting-ip")?.trim();
  if (!ip && process.env.ADMIN_TRUST_X_FORWARDED_FOR === "true") {
    ip = h.get("x-forwarded-for")?.split(",").at(-1)?.trim();
  }
  return ip ? { "x-forwarded-for": ip } : {};
}

/** A client for the routes that need no credentials (login and its second step). */
export async function publicClient() {
  return createAdminClient(apiUrl(), undefined, await forwardedFor(), apiFetch());
}

/**
 * A client acting as the signed-in staff member; without a session, to the login page.
 * A session the API no longer accepts (expired, revoked, user disabled) answers 401 to
 * any call: then the user goes to `/session-expired`, which clears the cookie, and
 * after signing in again comes back to the page they were on.
 */
export async function sessionClient() {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) redirect("/login");
  // On Workers (through the API's binding) reads aren't cached across requests: an
  // isolate that didn't see a write would keep serving the old read (design note 0025,
  // §4). Elsewhere one server sees every write, so its cache stays right.
  const bound = apiFetch();
  const client = createAdminClient(apiUrl(), token, {}, bound ?? sessionFetch(token));
  client.use({
    async onResponse({ response }) {
      if (response.status === 401) await sessionExpired();
      return undefined;
    },
  });
  return client;
}

/**
 * Sends the user to sign in again. Pages can't change cookies while rendering, so a
 * route handler (`app/session-expired/route.ts`) clears them. The way back is the
 * page the request came from (for a server action, the page it ran on).
 */
async function sessionExpired(): Promise<never> {
  let back = "/";
  const referer = (await headers()).get("referer");
  if (referer) {
    try {
      const url = new URL(referer);
      back = url.pathname + url.search;
    } catch {
      // Not a URL: back to the start.
    }
  }
  redirect(`/session-expired?next=${encodeURIComponent(safeNext(back))}`);
}

/**
 * A path on this admin to go to after signing in, or "/". Only plain local paths: never
 * another origin (`//evil.example`, `/\\evil.example`), nor the login pages themselves.
 */
export function safeNext(path: string | null | undefined): string {
  if (!path || !path.startsWith("/") || path.startsWith("//") || path.startsWith("/\\")) {
    return "/";
  }
  if (path.startsWith("/login") || path.startsWith("/session-expired")) return "/";
  return path;
}

/**
 * Where to land after signing in with `token`: `next`, except that an empty store (no
 * market and no product) starts at the setup wizard (design note 0024, AU11). A read the
 * user's scopes don't allow counts as not empty, so nobody is sent somewhere they can't
 * act.
 */
export async function landing(token: string, next: string): Promise<string> {
  if (next !== "/") return next;
  const client = createAdminClient(apiUrl(), token, {}, apiFetch());
  const { data } = await client.GET("/admin/store");
  return data && !data.setup.finished_at ? "/setup" : next;
}

/** Remembers where to go after the next login, for a few minutes. */
export async function rememberNext(path: string) {
  (await cookies()).set(NEXT_COOKIE, safeNext(path), { ...cookieBase, maxAge: 15 * 60 });
}

/** Where to go now that the user signed in: the remembered page, once, or "/". */
export async function takeNext(): Promise<string> {
  const jar = await cookies();
  const next = safeNext(jar.get(NEXT_COOKIE)?.value);
  jar.delete(NEXT_COOKIE);
  return next;
}

const cookieBase = {
  httpOnly: true,
  sameSite: "lax" as const,
  secure: process.env.NODE_ENV === "production",
  path: "/",
};

export async function startSession(session: AdminSchemas["SessionResponse"]) {
  const jar = await cookies();
  jar.set(SESSION_COOKIE, session.token, { ...cookieBase, expires: new Date(session.expires_at) });
  jar.delete(CHALLENGE_COOKIE);
}

export async function endSession() {
  const jar = await cookies();
  jar.delete(SESSION_COOKIE);
  jar.delete(CHALLENGE_COOKIE);
}

export async function holdChallenge(challenge: AdminSchemas["ChallengeResponse"]) {
  (await cookies()).set(CHALLENGE_COOKIE, challenge.token, {
    ...cookieBase,
    expires: new Date(challenge.expires_at),
  });
}

export async function heldChallenge(): Promise<string | undefined> {
  return (await cookies()).get(CHALLENGE_COOKIE)?.value;
}

/**
 * The signed-in staff member, for pages behind the login, once per request (the layout's
 * user menu and providers and the page share it). A session the API no longer accepts
 * goes to `/session-expired` (`sessionClient`).
 */
export const requireStaff = cache(async (): Promise<AdminSchemas["MeResponse"]> => {
  const client = await sessionClient();
  const { data, response } = await client.GET("/admin/me");
  if (data) return data;
  throw new Error(`GET /admin/me failed with ${response.status}`);
});

/**
 * The data of an API call a page needs. A missing resource goes to the 404 page (a
 * session the API no longer accepts never gets here: `sessionClient` redirects);
 * anything else is a bug or an outage, so it throws to the error page.
 */
export function loaded<T>(result: {
  data?: T;
  error?: ApiError;
  response: Response;
}): T {
  if (result.data !== undefined) return result.data;
  if (result.response.status === 404) notFound();
  throw new Error(
    `${result.response.url} failed with ${result.response.status} ${result.error?.code ?? ""}`,
  );
}

/** A message for the person at the screen, by the API's stable error code. */
export function describe(error: ApiError): string {
  switch (error.code) {
    case "invalid_credentials":
      return "Wrong email or password.";
    case "invalid_mfa_code":
      return "That code didn't work. Check your authenticator app and try again.";
    case "mfa_locked":
      return "Too many wrong codes. Try again in a few minutes.";
    case "invalid_token":
      return "That sign-in expired. Start again.";
    case "handle_taken":
      return "Another product already uses that handle. Pick a different one.";
    case "channel_name_taken":
      return "Another channel already has that name.";
    case "sku_taken":
      return "Another variant already uses that SKU. SKUs are unique across the store.";
    case "duplicate_option_combination":
      return "Another variant already has these option values.";
    case "option_value_in_use":
      return "Variants still use the values you removed.";
    case "duplicate_option_name":
      return "Two options have the same name.";
    case "duplicate_option_value":
      return "An option has the same value twice.";
    case "empty_option":
      return "Every option needs at least one value.";
    case "no_variants":
      return "A product keeps at least one variant.";
    case "too_many_variants":
      return "A product has at most 2,048 variants.";
    case "too_many_media":
      return "A product has at most 250 media items.";
    case "asset_in_use":
      return "This file is used on products. Remove it from them first.";
    case "asset_not_ready":
      return "That file hasn't finished uploading.";
    case "asset_kind_not_allowed":
      return "Products take images and videos; a variant shows an image.";
    case "country_in_other_zone":
      return "A country you listed is already in another zone. A country ships from one zone.";
    case "category_handle_taken":
      return "Another category has that handle. Handles are unique across categories.";
    case "category_cycle":
      return "A category can't move under itself or one of its subcategories.";
    case "category_too_deep":
      return `${error.error.charAt(0).toUpperCase()}${error.error.slice(1)}.`;
    case "category_has_children":
      return "This category has subcategories. Move or delete them first.";
    case "category_parent_not_found":
      return "That parent category no longer exists. Reload the page.";
    case "category_not_found":
      return "That category no longer exists. Reload the page.";
    case "invalid_country_code":
      return "Countries are two-letter codes, like HR or DE.";
    case "unknown_country_code":
      return "That isn't an ISO country code. Use the two letters ISO assigned, like HR or DE.";
    case "duplicate_country":
      return "A country is listed twice.";
    case "shipping_option_code_taken":
      return "Another option of this zone has that code.";
    case "invalid_shipping_option_code":
      return "Codes are lowercase letters, digits, '_' and '-', like standard or next-day.";
    case "invalid_shipping_rate":
      return `${error.error.charAt(0).toUpperCase()}${error.error.slice(1)}.`;
    case "too_many_shipping_options":
      return "A zone has at most 50 options.";
    case "invalid_shipping_provider_key":
      return "Keys are app_ followed by lowercase letters, digits and '_', like app_courier.";
    case "shipping_provider_exists":
      return "A rate app with that key already exists.";
    case "invalid_app_key":
      return "Keys are app_ followed by lowercase letters, digits and '_', like app_erp.";
    case "app_exists":
      return "An app with that key already exists.";
    case "invalid_subscription":
      return "Events are types like order.placed, a kind like product.*, or * for all.";
    case "too_many_subscriptions":
      return "An app subscribes to at most 50 event types.";
    case "invalid_timeout":
      return "The webhook timeout is between 1,000 and 30,000 milliseconds.";
    case "provider_app_not_found":
      return "There's no app with that key. Add the app first.";
    case "invalid_callback_url":
      return "The callback URL must start with https:// (http:// only for localhost).";
    case "invalid_callback_timeout":
      return "The timeout is between 100 and 10,000 milliseconds.";
    case "shipping_provider_failed":
    case "shipping_provider_timeout":
      return `The rate app didn't answer properly: ${error.error}.`;
    case "field_required":
    case "field_too_long":
      return error.error.charAt(0).toUpperCase() + error.error.slice(1) + ".";
    case "missing_scope":
      return "Your role doesn't allow this. Ask the store owner for access.";
    case "unsupported_media_type":
      return "That file type isn't supported. Use JPEG, PNG, WebP, AVIF, GIF, MP4, WebM, MOV or PDF.";
    case "file_too_large":
      return "That file is too large: images up to 20 MB, videos up to 1 GB, PDFs up to 100 MB.";
    case "upload_mismatch":
      return "The file's contents don't match its type. Export it again and retry.";
    case "media_not_configured":
      return "File storage isn't set up for this store yet.";
    case "over_fulfilled": {
      const left = (error.details as { unfulfilled?: number } | null)?.unfulfilled;
      return left === undefined
        ? "That's more than is left to ship."
        : `That's more than is left to ship: ${left} of that item.`;
    }
    case "order_has_fulfillments":
      return "Take back the order's shipments first, then cancel it.";
    case "order_cancelled":
      return "This order is cancelled.";
    case "fulfillment_cancelled":
      return "This shipment was taken back.";
    case "fulfillment_has_no_lines":
      return "Choose at least one item to ship.";
    case "duplicate_fulfillment_line":
      return "An item is listed twice.";
    case "invalid_tracking_url":
      return "The tracking link must start with https:// or http://.";
    case "too_much_tracking":
      return "A shipment has at most 10 tracking numbers.";
    case "location_not_found":
      return "That location doesn't exist any more. Reload and pick another.";
    case "concurrent_modification":
      return "Someone else changed this at the same time. Reload and try again.";
    case "country_in_other_market":
      return "A country you chose is already in another market. A country belongs to one market.";
    case "no_countries":
      return "A market needs at least one country.";
    case "last_country":
      return "A market keeps at least one country. Add another before removing this one.";
    case "catalog_exists":
      return "This market already has a catalog.";
    case "price_list_currency_mismatch":
      return "A market's price list must be in the market's currency.";
    case "fixed_price_currency_mismatch":
      return "A fixed price must be in the price list's currency.";
    case "duplicate_fixed_price":
      return "A variant is listed twice.";
    case "invalid_basis_points":
      return "A discount can't be more than 100%.";
    case "store_name_empty":
      return "The store needs a name.";
    case "store_name_too_long":
      return "The store name can be at most 100 characters.";
    case "store_description_too_long":
      return "The description can be at most 500 characters.";
    case "unknown_timezone":
      return "Choose a time zone from the list.";
    case "invalid_language":
      return "Write the language as a BCP 47 tag, like en, en-GB or sr-Latn.";
    case "invalid_email":
      return "That isn't a valid email address.";
    case "invalid_address":
      return "An address needs a last name, a first line and a city, each up to 255 characters.";
    case "rate_limited": {
      const secs = (error.details as { retry_after_secs?: number } | null)?.retry_after_secs;
      return secs ? `Too many attempts. Try again in ${secs} seconds.` : "Too many attempts.";
    }
    default:
      return error.error;
  }
}

/** Sales channels' names by ID (design note 0020). Empty without `channels:read`, so a
 * page that only labels channels still shows, with their IDs. */
export async function channelNames(
  client: Awaited<ReturnType<typeof sessionClient>>,
): Promise<Map<string, string>> {
  const { data } = await client.GET("/admin/channels");
  return new Map((data?.items ?? []).map((c) => [c.id, c.name]));
}

/**
 * Minor units by currency (`GET /admin/currencies`, design note 0021), fetched once per
 * request: React's `cache` shares one result between the layout, which provides it to
 * client components (`useCurrencies`), and the page's own server code.
 */
export const loadCurrencies = cache(async (): Promise<Currencies> => {
  const client = await sessionClient();
  const { items } = loaded(await client.GET("/admin/currencies"));
  return Object.fromEntries(items.map((c) => [c.code, c.minor_units]));
});

/**
 * The store's time zone (IANA, from `GET /admin/store`), once per request: times show in
 * it. UTC when the staff member can't read the store or it has none.
 */
export const loadTimeZone = cache(async (): Promise<string> => {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/store");
  return data?.timezone ?? "UTC";
});

/**
 * How far the setup guide got (design note 0024, AU11), once per request: the guide, its
 * nav item and the home page's checklist show only until it's finished. `null` when the
 * staff member can't read the store: then nothing about the guide shows.
 */
export const loadSetup = cache(
  async (): Promise<{ step: string | null; isFinished: boolean } | null> => {
    const client = await sessionClient();
    const { data } = await client.GET("/admin/store");
    if (!data) return null;
    return { step: data.setup.step ?? null, isFinished: Boolean(data.setup.finished_at) };
  },
);

/**
 * Every country an address or market can name (`GET /admin/countries`), once per
 * request, for pickers.
 */
export const loadCountries = cache(async (): Promise<Country[]> => {
  const client = await sessionClient();
  return loaded(await client.GET("/admin/countries")).items;
});
