// Which language the admin speaks to someone, from what their browser sent, and the
// locale it formats in. Plain functions, shared by the server and the browser.

import { canonicalLanguage, matchLanguage, parseAcceptLanguage } from "@hyphacommerce/i18n";

import { PSEUDO } from "./pseudo.ts";

/** The source language: every key exists in it, and it's what nobody asked for gets. */
export const DEFAULT_LANGUAGE = "en";

/** The cookie the profile's language select writes. */
export const LANGUAGE_COOKIE = "hypha_admin_language";

/**
 * The language for a request: the one chosen in the profile (the cookie) when the admin
 * has it, else the first language of the browser's `Accept-Language` that `available`
 * has (or has a nearer form of: `sr` reads `sr-Latn`), else English. Always one of
 * `available`, in its spelling.
 */
export function pickLanguage(
  wanted: { cookie?: string | null; acceptLanguage?: string | null },
  available: readonly string[],
): string {
  const chosen = wanted.cookie ? canonicalLanguage(wanted.cookie) : undefined;
  const preferred = [...(chosen ? [chosen] : []), ...parseAcceptLanguage(wanted.acceptLanguage)];
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
