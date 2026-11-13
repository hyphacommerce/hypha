// Server-only: the language of the request. One place decides it, for the root layout's
// `<html lang dir>` and providers, the pages behind the login and the ones before it
// (login, invite, password reset), which have no session to ask.

import { cookies, headers } from "next/headers";

import { LANGUAGES } from "./messages";
import { LANGUAGE_COOKIE, pickLanguage } from "./pick-language.ts";
import { PSEUDO } from "./pseudo.ts";

/**
 * Whether the pseudo-language is offered: only a development server started with
 * `HYPHA_I18N_PSEUDO=1`.
 */
function isPseudoOffered(): boolean {
  return process.env.NODE_ENV === "development" && process.env.HYPHA_I18N_PSEUDO === "1";
}

/** The languages a staff member can choose: the ones with catalogs, English first. */
export function offeredLanguages(): readonly string[] {
  return isPseudoOffered() ? [...LANGUAGES, PSEUDO] : LANGUAGES;
}

/**
 * The request's language: the staff member's choice (the cookie their profile writes),
 * else the browser's `Accept-Language` matched against the catalogs, else English. The
 * admin's language is the person's, never the store's content language.
 */
export async function getLanguage(): Promise<string> {
  const [jar, sent] = await Promise.all([cookies(), headers()]);
  return pickLanguage(
    { cookie: jar.get(LANGUAGE_COOKIE)?.value, acceptLanguage: sent.get("accept-language") },
    offeredLanguages(),
  );
}

/**
 * Keeps `language` as the staff member's choice on this browser, for a year. Only in a
 * server action or a route handler (pages can't set cookies). `false` for a language the
 * admin doesn't offer, which changes nothing.
 */
export async function rememberLanguage(language: string): Promise<boolean> {
  if (!offeredLanguages().includes(language)) return false;
  (await cookies()).set(LANGUAGE_COOKIE, language, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 365 * 24 * 60 * 60,
  });
  return true;
}
