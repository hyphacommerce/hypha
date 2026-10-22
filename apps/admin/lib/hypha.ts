// Server-only: how the admin's server talks to the Hypha admin API. The staff session
// lives in an httpOnly cookie that page scripts can't read; only code here sends it to
// the API. Never import this from a "use client" file.

import { apiFetch } from "#api-fetch";
import { type AdminSchemas, type ApiError, createAdminClient } from "@hypha/sdk/admin";
import { cookies, headers } from "next/headers";
import { forbidden, notFound, redirect } from "next/navigation";

import type { Currencies } from "./format";
import { describeProblem, problemOf } from "./notifications";
import type { Country } from "./regions";
import { safeNext } from "./safe-next";

const SESSION_COOKIE = "hypha_admin_session";
/** A pending login's second-factor challenge, for the few minutes it lives. */
const CHALLENGE_COOKIE = "hypha_admin_challenge";
/** Where to go after signing in again, set when a session expired. */
const NEXT_COOKIE = "hypha_admin_next";
/** The request's own path and query, set by `proxy.ts` on the pages it matches. */
export const PAGE_HEADER = "x-hypha-admin-page";

function apiUrl(): string {
  const url = process.env.HYPHA_API_URL;
  if (!url) {
    throw new Error("HYPHA_API_URL is not set (see apps/admin/.env.example)");
  }
  return url;
}

/**
 * The API's address as apps and browsers reach it (an app's install route takes it):
 * `HYPHA_PUBLIC_API_URL`, else `HYPHA_API_URL` when the admin calls the API at its
 * public address anyway.
 */
export function publicApiUrl(): string {
  return (process.env.HYPHA_PUBLIC_API_URL ?? apiUrl()).replace(/\/+$/, "");
}

/**
 * The user's address, passed on so Hypha's per-address rate limits count the user, not
 * the admin server. Hypha only believes it when the admin's address is in
 * `HYPHA_TRUSTED_PROXIES`.
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

/**
 * The browser's `User-Agent`, passed on so a session opened by a login or an invite
 * shows which browser it is (`GET /admin/me/sessions`), not the admin server's.
 */
async function forwardedUserAgent(): Promise<Record<string, string>> {
  const agent = (await headers()).get("user-agent")?.trim();
  return agent ? { "user-agent": agent } : {};
}

/** A client for the routes that need no credentials (login and its second step). */
export async function publicClient() {
  const passedOn = { ...(await forwardedFor()), ...(await forwardedUserAgent()) };
  return createAdminClient(apiUrl(), undefined, passedOn, apiFetch());
}

/**
 * A client acting as the signed-in staff member; without a session, to the login page.
 * A session the API no longer accepts (expired, revoked, user disabled) answers 401 to
 * any call: then the user goes to `/session-expired`, which clears the cookie, and
 * after signing in again comes back to the page they were on.
 *
 * Nothing is cached across requests: every admin page is dynamic and reads the API
 * fresh, so what staff see is what the store holds now, on every host alike. Within a
 * request, the `perRequest` loaders below make the layout and the page share a read.
 */
export async function sessionClient() {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) redirect("/login");
  const client = createAdminClient(apiUrl(), token, {}, apiFetch());
  client.use({
    async onResponse({ request, response }) {
      if (response.status === 401) await sessionExpired();
      // A write (in a server action) may change what this request already read: the
      // page it re-renders reads again. A template preview is a POST that writes nothing.
      if (request.method !== "GET" && !isPreview(request.url)) await forgetRequestReads();
      return undefined;
    },
  });
  return client;
}

/** Whether `url` renders a notification template's preview, which changes nothing. */
function isPreview(url: string): boolean {
  return /\/admin\/notification-templates\/[a-z_]+\/preview$/.test(new URL(url).pathname);
}

/**
 * The current reads of each request, by its headers object: a fresh object per request,
 * replaced after a write so the reads made before it are no longer found.
 */
const requestReads = new WeakMap<object, object>();

async function readsOfRequest(): Promise<object> {
  const scope = await headers();
  let reads = requestReads.get(scope);
  if (!reads) {
    reads = {};
    requestReads.set(scope, reads);
  }
  return reads;
}

/**
 * `load`, run at most once per request: the layout's parts, the page and its parts
 * share one answer. React's `cache` can't do this here: vinext renders the layout in
 * more than one pass, each with a cache of its own, so the same read went to the API
 * two or three times per page. The request's headers object (one per request, in Next.js
 * and vinext alike) keys the memo instead, so nothing outlives the request.
 */
export function perRequest<T>(load: () => Promise<T>): () => Promise<T> {
  // Each loader keeps its own answers, so each map holds one type.
  const answers = new WeakMap<object, Promise<T>>();
  return async () => {
    const reads = await readsOfRequest();
    let read = answers.get(reads);
    if (!read) {
      read = load();
      answers.set(reads, read);
    }
    return read;
  };
}

/** Forgets this request's `perRequest` reads, after a write. */
async function forgetRequestReads() {
  requestReads.delete(await headers());
}

/**
 * Sends the user to sign in again. Pages can't change cookies while rendering, so a
 * route handler (`app/session-expired/route.ts`) clears them. The way back is the
 * page the request came from (for a server action, the page it ran on).
 */
async function sessionExpired(): Promise<never> {
  let back = "/";
  const h = await headers();
  // The page itself, where `proxy.ts` names it (pages reached from another site, like
  // the consent page an app sends the browser to, have no referer of ours).
  const page = h.get(PAGE_HEADER);
  const referer = h.get("referer");
  if (page) {
    back = page;
  } else if (referer) {
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
 * Where to land after signing in with `token`: `next`, except that an empty store (no
 * market and no product) starts at the setup wizard. A read the user's scopes don't
 * allow counts as not empty, so nobody is sent somewhere they can't act.
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
export const requireStaff = perRequest(async (): Promise<AdminSchemas["MeResponse"]> => {
  const client = await sessionClient();
  const { data, response } = await client.GET("/admin/me");
  if (data) return data;
  throw new Error(`GET /admin/me failed with ${response.status}`);
});

/**
 * Whether an API answer is a `403 missing_scope`: the user's role lacks what the call
 * needs, which pages show as a notice rather than an error.
 */
export function isMissingScope(result: { error?: ApiError; response: Response }): boolean {
  return result.response.status === 403 && result.error?.code === "missing_scope";
}

/**
 * The data of an API call a page needs. A missing resource goes to the 404 page, and a
 * read the user's role doesn't allow to the no-access notice (`(admin)/forbidden.tsx`)
 * inside the frame, so the rest of the admin stays usable (a session the API no longer
 * accepts never gets here: `sessionClient` redirects); anything else is a bug or an
 * outage, so it throws to the error page.
 */
export function loaded<T>(result: { data?: T; error?: ApiError; response: Response }): T {
  if (result.data !== undefined) return result.data;
  if (result.response.status === 404) notFound();
  if (isMissingScope(result)) forbidden();
  throw new Error(
    `${result.response.url} failed with ${result.response.status} ${result.error?.code ?? ""}`,
  );
}

/** A number among an error's `details`, if the API sent one under `key`. */
function numberDetail(details: unknown, key: string): number | undefined {
  if (typeof details !== "object" || details === null || !(key in details)) return undefined;
  const value: unknown = Reflect.get(details, key);
  return typeof value === "number" ? value : undefined;
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
    case "wrong_password":
      return "That isn't your current password.";
    case "password_too_short":
      return "Use at least 8 characters for the password.";
    case "password_too_long":
      return "The password can be at most 128 characters.";
    case "mfa_enabled":
      return "Two-factor authentication is already on.";
    case "mfa_not_enabled":
      return "Two-factor authentication is off.";
    case "mfa_setup_not_started":
      return "That setup expired. Start setting up the authenticator again.";
    case "session_required":
      return "Only a signed-in staff member can do this.";
    case "user_exists":
      return "Someone with that email is already on the team.";
    case "user_not_found":
      return "That staff member no longer exists. Reload the page.";
    case "role_not_found":
      return "That role no longer exists. Reload the page.";
    case "invite_not_found":
      return "That invite was already accepted or withdrawn. Reload the page.";
    case "session_not_found":
      return "That device is already signed out. Reload the page.";
    case "grant_not_narrower":
      return "Access can only be reduced here. To add a scope, the app asks again and staff approve it.";
    case "app_not_installed":
      return "This app isn't installed, so it has no access to reduce.";
    case "app_unavailable":
      return "The app didn't answer. Try again in a moment.";
    case "app_action_has_point":
      return "This action runs through its payment route, not as an app action.";
    case "invalid_extension_context":
      return "The app's action doesn't fit here. Reload the page.";
    case "oauth_request_not_found":
      return "That authorization request doesn't exist. Start the installation again from the app.";
    case "oauth_request_expired":
      return "This request expired: requests wait ten minutes. Start again from the app.";
    case "oauth_request_decided":
      return "This request was already answered.";
    case "manifest_invalid":
      return "The manifest isn't valid.";
    case "manifest_unreachable":
      return "The app's manifest couldn't be read.";
    case "app_installed_from_url":
      return "An app with this key is installed from its own site; a private app needs another key.";
    case "app_installed_from_other_url":
      return "An app with this key is installed from another manifest URL, so this request can't replace it.";
    case "app_is_private":
      return "A private app changes by uploading its manifest again.";
    case "payment_refused": {
      const reason =
        typeof error.details === "object" && error.details !== null
          ? Reflect.get(error.details, "reason")
          : undefined;
      return typeof reason === "string"
        ? `The provider refused: ${reason}`
        : "The provider refused.";
    }
    case "payment_action_unsupported":
      return "This payment's provider can't do that.";
    case "invalid_refund_amount":
      return "That's more than is left to refund.";
    case "role_name_taken":
      return "Another role already has that name.";
    case "role_scopes_required":
      return "A role needs at least one scope.";
    case "owner_role_fixed":
      return "The owner role always has every scope; it can't be changed.";
    case "last_owner":
      return "The store keeps at least one active owner. Make someone else an owner first.";
    case "not_allowed_to_grant":
      return "You can only give away scopes your own role has.";
    case "notification_failed":
      return "The email couldn't be sent. Check the email settings under Notifications, then try again.";
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
      const left = numberDetail(error.details, "unfulfilled");
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
    case "payment_method_in_use":
      return "An order is still waiting for a payment by this method. Turn the method off instead, so shoppers stop seeing it.";
    case "payment_method_not_found":
      return "That payment method no longer exists. Reload the page.";
    case "instructions_too_long":
      return "Instructions can be at most 2,000 characters.";
    case "invalid_payment_provider":
    case "payment_provider_unknown":
      return "This build of Hypha doesn't have that payment provider.";
    case "invalid_payment_settings":
      return `The provider refused its settings: ${error.error}.`;
    case "invalid_payment_transition":
      return "The payment can't change that way any more. Reload the order.";
    case "template_invalid": {
      const problem = problemOf(error.details);
      return problem
        ? `The template doesn't work. ${describeProblem(problem)}`
        : "The template doesn't work. Check its variables and tags.";
    }
    case "template_too_complex":
      return "The template takes too long to render: a loop runs too many times.";
    case "template_required":
      return "This email can't be turned off: the layout, invites and password resets are always sent.";
    case "template_too_large":
      return `${error.error.charAt(0).toUpperCase()}${error.error.slice(1)}. Shorten it.`;
    case "template_part_required":
      return "An email needs a subject and an HTML body.";
    case "template_part_not_allowed":
      return "The layout only has HTML: leave its subject and plain text empty.";
    case "template_unknown":
      return "There's no such email template. Reload the page.";
    case "invalid_colour":
      return "The accent colour is six hex digits after a #, like #1a2b3c.";
    case "invalid_order_url":
      return "The order link must be an http(s) URL with {order} in it, and no placeholders but {order} and {token}.";
    case "too_many_recipients":
      return "At most 10 staff recipients.";
    case "footer_too_long":
      return "The footer can be at most 1,000 characters.";
    case "email_not_configured":
      return "Emails are only written to the server log, so a test would reach nobody. Set up a sender under [email] in hypha.config.toml.";
    case "email_unavailable":
      return `The email provider couldn't be reached. Try again in a moment. (${error.error})`;
    case "email_refused":
      return `The email provider refused the message: ${error.error}`;
    case "notification_not_found":
      return "That email record no longer exists. Reload the page.";
    case "notification_not_resendable":
      return "Invites and password resets can't be resent: send a new invite or reset instead.";
    case "staff_only":
      return "Only a signed-in staff member can send a test: it goes to your own address.";
    case "asset_not_found":
      return "That file is no longer in the media library.";
    case "asset_not_public":
      return "That image is private. Emails can only show public images: change its visibility on the Media page.";
    case "custom_fields_invalid":
      return "Some custom fields aren't valid: see the marked fields.";
    case "custom_field_stale":
      return "Someone else changed a custom field meanwhile. Reload to see it, then try again.";
    case "custom_field_read_only":
      return "One of these fields is read-only: its app manages it.";
    case "custom_field_exists":
      return "Another field of this kind of record already has that key.";
    case "custom_field_managed":
      return "An app or spore declares this field, so it can't be changed here.";
    case "custom_field_values_invalid": {
      const n = numberDetail(error.details, "count");
      return `${n === undefined ? "Some" : n.toLocaleString("en")} stored ${n === 1 ? "value breaks" : "values break"} the new rules. Fix them first, or loosen the rules.`;
    }
    case "custom_field_definition_not_found":
      return "That field no longer exists. Reload the page.";
    case "custom_field_unknown":
      return "A filter names a custom field that doesn't exist any more.";
    case "custom_field_not_filterable":
      return "A filter names a custom field that isn't filterable.";
    case "personal_not_allowed":
      return "Only customers' and orders' fields can hold personal data.";
    case "unique_not_allowed":
      return "Only single text, integer and URL fields can be unique.";
    case "filterable_not_allowed":
      return "Multi-line text, rich text and JSON fields can't be filters.";
    case "too_many_filterable_custom_fields":
      return "A kind of record has at most 50 filterable fields.";
    case "too_many_custom_fields":
      return "There are too many fields for this kind of record.";
    case "invalid_custom_field_key":
      return "The key is 2 to 64 lowercase letters, digits and _, starting with a letter.";
    case "invalid_field_type":
      return "That isn't a field type.";
    case "invalid_validation":
    case "validation_required":
    case "validation_not_allowed":
    case "description_too_long":
      return `${error.error.charAt(0).toUpperCase()}${error.error.slice(1)}.`;
    case "rate_limited": {
      const secs = numberDetail(error.details, "retry_after_secs");
      return secs ? `Too many attempts. Try again in ${secs} seconds.` : "Too many attempts.";
    }
    default:
      return error.error;
  }
}

/** Sales channels' names by ID. Empty without `channels:read`, so a page that only labels
 * channels still shows, with their IDs. */
export async function channelNames(
  client: Awaited<ReturnType<typeof sessionClient>>,
): Promise<Map<string, string>> {
  const { data } = await client.GET("/admin/channels");
  return new Map((data?.items ?? []).map((c) => [c.id, c.name]));
}

/** Locations' names by ID. Empty without `inventory:read`. */
export async function locationNames(
  client: Awaited<ReturnType<typeof sessionClient>>,
): Promise<Map<string, string>> {
  const { data } = await client.GET("/admin/locations");
  return new Map((data?.items ?? []).map((l) => [l.id, l.name]));
}

/**
 * Minor units by currency (`GET /admin/currencies`), fetched once per request: the
 * layout, which provides it to client components (`useCurrencies`), and the page's own
 * server code share one result.
 */
export const loadCurrencies = perRequest(async (): Promise<Currencies> => {
  const client = await sessionClient();
  const { items } = loaded(await client.GET("/admin/currencies"));
  return Object.fromEntries(items.map((c) => [c.code, c.minor_units]));
});

/**
 * The store (`GET /admin/store`), once per request: the layout's time zone and setup
 * guide and the page share it. `null` when the staff member can't read it.
 */
export const loadStore = perRequest(async (): Promise<AdminSchemas["StoreResponse"] | null> => {
  const client = await sessionClient();
  const result = await client.GET("/admin/store");
  if (isMissingScope(result)) return null;
  return loaded(result);
});

/** The store, for pages about it: one the staff member can't read is the no-access notice. */
export async function requireStore(): Promise<AdminSchemas["StoreResponse"]> {
  const store = await loadStore();
  if (!store) forbidden();
  return store;
}

/**
 * The store's time zone (IANA, from the store): times show in it. UTC when the staff
 * member can't read the store or it has none.
 */
export async function loadTimeZone(): Promise<string> {
  return (await loadStore())?.timezone ?? "UTC";
}

/**
 * How far the setup guide got: the guide, its nav item and the home page's checklist
 * show only until it's finished. `null` when the staff member can't read the store:
 * then nothing about the guide shows.
 */
export async function loadSetup(): Promise<{ step: string | null; isFinished: boolean } | null> {
  const store = await loadStore();
  if (!store) return null;
  return { step: store.setup.step ?? null, isFinished: Boolean(store.setup.finished_at) };
}

/**
 * Every country an address or market can name (`GET /admin/countries`), once per
 * request, for pickers.
 */
export const loadCountries = perRequest(async (): Promise<Country[]> => {
  const client = await sessionClient();
  return loaded(await client.GET("/admin/countries")).items;
});

/**
 * The store's payment methods in the order shoppers see them (`GET
 * /admin/payment-methods`), once per request. `null` when the staff member lacks
 * `payments:read`, so pages that only mention payments still show.
 */
export const loadPaymentMethods = perRequest(
  async (): Promise<AdminSchemas["PaymentMethodResponse"][] | null> => {
    const client = await sessionClient();
    const result = await client.GET("/admin/payment-methods");
    if (result.response.status === 403) return null;
    return loaded(result).items;
  },
);

/**
 * The notification settings (`GET /admin/notification-settings`), once per request: the
 * settings page and a template's editor (whether a test can be sent) share it. `null`
 * without `notifications:read`.
 */
export const loadNotificationSettings = perRequest(
  async (): Promise<AdminSchemas["NotificationSettingsResponse"] | null> => {
    const client = await sessionClient();
    const result = await client.GET("/admin/notification-settings");
    if (result.response.status === 403) return null;
    return loaded(result);
  },
);
