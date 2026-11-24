// Which language the admin speaks to someone, from what their browser sent, and the
// locale it formats in. Plain functions, shared by the server and the browser.

import { canonicalLanguage, matchLanguage, parseAcceptLanguage } from "@hyphacommerce/i18n";

import { PSEUDO } from "./pseudo.ts";

/** The source language: every key exists in it, and it's what nobody asked for gets. */
export const DEFAULT_LANGUAGE = "en";

/**
 * The cookie the profile's language select writes: the choice on this browser, which the
 * pages before the login read, and the only place the pseudo-language is kept.
 */
export const LANGUAGE_COOKIE = "hypha_admin_language";

/**
 * The language for a request, from the first of these the admin has (or has a nearer
 * form of: `sr` reads `sr-Latn`): the signed-in staff member's own (`users.language`),
 * the one chosen on this browser (the cookie: the pages before the login have no user),
 * the browser's `Accept-Language` in its order, else English. Always one of `available`,
 * in its spelling.
 *
 * The pseudo-language is the exception: it's never saved on the user, so where the
 * cookie names it it comes first, and where it isn't offered the cookie is passed over.
 */
export function pickLanguage(
  wanted: { user?: string | null; cookie?: string | null; acceptLanguage?: string | null },
  available: readonly string[],
): string {
  const user = wanted.user ? canonicalLanguage(wanted.user) : undefined;
  const cookie = wanted.cookie ? canonicalLanguage(wanted.cookie) : undefined;
  // Not offered, the pseudo-language is no choice at all (it would read as English).
  const chosen =
    cookie !== PSEUDO ? [user, cookie] : available.includes(PSEUDO) ? [PSEUDO, user] : [user];
  const preferred = [
    ...chosen.filter((tag) => tag !== undefined),
    ...parseAcceptLanguage(wanted.acceptLanguage),
  ];
  return matchLanguage(preferred, available) ?? DEFAULT_LANGUAGE;
}

/**
 * The locale a language's numbers, money and dates are written in: the language itself,
 * except that the admin's English is the British one, as it always wrote them (day
 * first, "15 Oct 2026, 09:30 CEST", "€18.00", "US$18.00"). The pseudo-language formats
 * as English. This is a translator's `locale`, which every format takes.
 */
export function formatLocale(language: string): string {
  return language === DEFAULT_LANGUAGE || language === PSEUDO ? "en-GB" : language;
}
