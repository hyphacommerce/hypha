// Dates and names as a locale writes them. Pure: every function takes the locale (a
// translator's `locale`), never assumes one.

import { withEnglish } from "@hyphacommerce/i18n";

/** Formatters by locale, built once: a page formats many dates. */
const dateFormats = new Map<string, Intl.DateTimeFormat>();

/** A day as the locale writes it out: "18 October 2026", "October 18, 2026". */
export function formatDate(value: string | Date, locale: string): string {
  let format = dateFormats.get(locale);
  if (!format) {
    format = new Intl.DateTimeFormat(withEnglish(locale), { dateStyle: "long" });
    dateFormats.set(locale, format);
  }
  return format.format(typeof value === "string" ? new Date(value) : value);
}

/**
 * A language's name in itself (`de` → "Deutsch", `sr-Latn` → "srpski (latinica)"), as
 * a language picker shows it: a reader finds their own language whatever the page is
 * in. Starts with a capital where the language has them; the tag where the runtime
 * doesn't know the language.
 */
export function languageName(language: string): string {
  try {
    const name = new Intl.DisplayNames(withEnglish(language), { type: "language" }).of(language);
    if (!name || name === language) return language;
    return name.charAt(0).toLocaleUpperCase(language) + name.slice(1);
  } catch {
    return language;
  }
}

/** A country's name in the locale's language where the runtime knows it, else
 * `fallback` (the API's English one). */
export function countryName(code: string, fallback: string, locale: string): string {
  try {
    return new Intl.DisplayNames(withEnglish(locale), { type: "region" }).of(code) ?? fallback;
  } catch {
    return fallback;
  }
}
