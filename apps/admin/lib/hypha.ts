// Server-only: how the admin's server talks to the Hypha admin API (design note 0011,
// P4). The staff session lives in an httpOnly cookie that page scripts can't read; only
// code here sends it to the API. Never import this from a "use client" file.

import { type AdminSchemas, type ApiError, createAdminClient } from "@hypha/sdk/admin";
import { cookies, headers } from "next/headers";
import { notFound, redirect } from "next/navigation";

const SESSION_COOKIE = "hypha_admin_session";
/** A pending login's second-factor challenge, for the few minutes it lives. */
const CHALLENGE_COOKIE = "hypha_admin_challenge";

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
 */
async function forwardedFor(): Promise<Record<string, string>> {
  const h = await headers();
  const ip = h.get("cf-connecting-ip") ?? h.get("x-forwarded-for")?.split(",")[0]?.trim();
  return ip ? { "x-forwarded-for": ip } : {};
}

/** A client for the routes that need no credentials (login and its second step). */
export async function publicClient() {
  return createAdminClient(apiUrl(), undefined, await forwardedFor());
}

/** A client acting as the signed-in staff member; without a session, to the login page. */
export async function sessionClient() {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) redirect("/login");
  return createAdminClient(apiUrl(), token);
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
 * The signed-in staff member, for pages behind the login. A session the API no longer
 * accepts (expired, revoked, user disabled) sends the user to the login page; the stale
 * cookie is replaced at the next login.
 */
export async function requireStaff(): Promise<AdminSchemas["MeResponse"]> {
  const client = await sessionClient();
  const { data, response } = await client.GET("/admin/me");
  if (data) return data;
  if (response.status === 401) redirect("/login");
  throw new Error(`GET /admin/me failed with ${response.status}`);
}

/**
 * The data of an API call a page needs. A session the API no longer accepts goes to the
 * login page and a missing resource to the 404 page; anything else is a bug or an
 * outage, so it throws to the error page.
 */
export function loaded<T>(result: {
  data?: T;
  error?: ApiError;
  response: Response;
}): T {
  if (result.data !== undefined) return result.data;
  if (result.response.status === 401) redirect("/login");
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
    case "concurrent_modification":
      return "Someone else changed this at the same time. Reload and try again.";
    case "rate_limited": {
      const secs = (error.details as { retry_after_secs?: number } | null)?.retry_after_secs;
      return secs ? `Too many attempts. Try again in ${secs} seconds.` : "Too many attempts.";
    }
    default:
      return error.error;
  }
}
