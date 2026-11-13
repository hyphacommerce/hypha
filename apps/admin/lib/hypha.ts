// Server-only: how the admin's server talks to the Hypha admin API. The staff session
// lives in an httpOnly cookie that page scripts can't read; only code here sends it to
// the API. Never import this from a "use client" file.

import { apiFetch } from "#api-fetch";
import { type AdminSchemas, type ApiError, createAdminClient } from "@hyphacommerce/sdk/admin";
import { cookies, headers } from "next/headers";
import { forbidden, notFound, redirect } from "next/navigation";

import type { Currencies } from "./format";
import { describeWith } from "./errors.ts";
import { getT } from "./i18n";
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
 * This request's `GET /admin/me`, once: `null` without a session cookie. It never
 * redirects, so the root layout can ask it on any page (`staffLanguage`), the ones
 * before the login included; `requireStaff` decides what a failed read means.
 */
const readStaff = perRequest(async () => {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) return null;
  return createAdminClient(apiUrl(), token, {}, apiFetch()).GET("/admin/me");
});

/**
 * The signed-in staff member, for pages behind the login, once per request (the layout's
 * user menu and providers and the page share it). Without a session, to the login page;
 * a session the API no longer accepts goes to `/session-expired`.
 */
export async function requireStaff(): Promise<AdminSchemas["MeResponse"]> {
  const read = await readStaff();
  if (!read) redirect("/login");
  if (read.data) return read.data;
  if (read.response.status === 401) await sessionExpired();
  throw new Error(`GET /admin/me failed with ${read.response.status}`);
}

/**
 * The signed-in staff member's own language (`users.language`), or `null`: none chosen,
 * no session, or a session the API refused. The language of a page is decided before it
 * knows whether it needs a session, so this asks without consequences.
 */
export async function staffLanguage(): Promise<string | null> {
  try {
    return (await readStaff())?.data?.user.language ?? null;
  } catch {
    // The API unreachable: the page's own reads will say so; its language needn't.
    return null;
  }
}

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

/**
 * A message for the person at the screen, in the request's language:
 * `return { error: await describe(error) }`.
 */
export async function describe(error: ApiError): Promise<string> {
  return describeWith(await getT(), error);
}

export { describeWith };

/** Sales channels' names by ID. Empty without `channels:read`, so a page that only labels
 * channels still shows, with their IDs. */
export async function channelNames(
  client: Awaited<ReturnType<typeof sessionClient>>,
): Promise<Map<string, string>> {
  const { data } = await client.GET("/admin/channels");
  return new Map((data?.items ?? []).map((c) => [c.id, c.name]));
}

/** Staff members' names by user ID. Empty without `users:read`, so a page that only names
 * people still shows, with their IDs. */
export async function staffNames(
  client: Awaited<ReturnType<typeof sessionClient>>,
): Promise<Map<string, string>> {
  const { data } = await client.GET("/admin/users");
  return new Map((data?.items ?? []).map((u) => [u.id, u.name]));
}

/**
 * The operator's settings for agents (`GET /admin/agent-settings`: on or off, the cap on
 * what they write), once per request. `null` without `agents:read`.
 */
export const loadAgentSettings = perRequest(
  async (): Promise<AdminSchemas["AgentSettingsResponse"] | null> => {
    const client = await sessionClient();
    const result = await client.GET("/admin/agent-settings");
    if (isMissingScope(result)) return null;
    return loaded(result);
  },
);

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
