// Language tags (BCP 47): reading what a browser asks for, matching it against what a
// store or an app has, text direction, formatting locales, and the storefront's URL
// segment.

/** The tag as BCP 47 spells it (`sr-latn` → `sr-Latn`), or undefined when it isn't one. */
export function canonicalLanguage(tag: string): string | undefined {
  try {
    return Intl.getCanonicalLocales(tag)[0];
  } catch {
    return undefined;
  }
}

/**
 * The languages of an `Accept-Language` header, most wanted first (by `q`, then by
 * position). Leaves out `*`, anything refused with `q=0` and what isn't a language tag.
 */
export function parseAcceptLanguage(header: string | null | undefined): string[] {
  if (!header) return [];
  const wanted: { tag: string; q: number }[] = [];
  for (const item of header.split(",")) {
    const [range = "", ...params] = item.split(";").map((part) => part.trim());
    if (range === "" || range === "*") continue;
    const tag = canonicalLanguage(range);
    if (tag === undefined) continue;
    let q = 1;
    for (const param of params) {
      const match = /^q\s*=\s*([0-9.]+)$/i.exec(param);
      if (match) q = Number(match[1]);
    }
    if (!(q > 0 && q <= 1)) continue;
    if (!wanted.some((other) => other.tag === tag)) wanted.push({ tag, q });
  }
  // Array.prototype.sort is stable: equal weights keep the header's order.
  return wanted.toSorted((a, b) => b.q - a.q).map((item) => item.tag);
}

/**
 * The first of `wanted` that `available` has, in `available`'s spelling: the exact tag,
 * then the tag with its last parts taken off one at a time (`de-CH` → `de`,
 * `sr-Latn-RS` → `sr-Latn` → `sr`), compared without case.
 *
 * Only when that finds nothing for any wanted tag does it widen: the first of `wanted`
 * whose language some available tag shares gets the first such tag (`sr` or `sr-RS`
 * → `sr-Latn`, `de` → `de-AT`). Two passes over all of `wanted`, so an exact or
 * shortened match for a later wanted tag beats a widened one for an earlier tag.
 */
export function matchLanguage(
  wanted: readonly string[],
  available: readonly string[],
): string | undefined {
  const spelled = new Map<string, string>();
  for (const tag of available) {
    const lower = tag.toLowerCase();
    if (!spelled.has(lower)) spelled.set(lower, tag);
  }
  for (const tag of wanted) {
    const parts = tag.trim().toLowerCase().split("-");
    while (parts.length > 0) {
      const found = spelled.get(parts.join("-"));
      if (found !== undefined) return found;
      parts.pop();
      // A single letter introduces an extension (`-u-…`, `-x-…`): it goes with its values.
      if (parts.at(-1)?.length === 1) parts.pop();
    }
  }
  for (const tag of wanted) {
    const primary = tag.trim().toLowerCase().split("-")[0];
    if (!primary) continue;
    const found = available.find((other) => other.toLowerCase().split("-")[0] === primary);
    if (found !== undefined) return found;
  }
  return undefined;
}

export type Direction = "ltr" | "rtl";

const RTL_SCRIPTS = new Set([
  "adlm",
  "arab",
  "hebr",
  "mand",
  "nkoo",
  "rohg",
  "samr",
  "syrc",
  "thaa",
]);
// Languages written right to left when the tag names no script.
const RTL_LANGUAGES = new Set([
  "ar", // Arabic
  "arc", // Aramaic
  "ckb", // Central Kurdish
  "dv", // Dhivehi
  "fa", // Persian
  "he", // Hebrew
  "iw", // Hebrew, as older software names it
  "ks", // Kashmiri
  "nqo", // N'Ko
  "ps", // Pashto
  "sd", // Sindhi
  "syr", // Syriac
  "ug", // Uyghur
  "ur", // Urdu
  "yi", // Yiddish
  "ji", // Yiddish, as older software names it
]);

/** The direction `language` is written in: by its script when the tag names one, else by the language. */
export function directionOf(language: string): Direction {
  const [primary = "", ...rest] = language.toLowerCase().split(/[-_]/);
  const script = rest.find((part) => part.length === 4 && /^[a-z]+$/.test(part));
  if (script !== undefined) return RTL_SCRIPTS.has(script) ? "rtl" : "ltr";
  return RTL_LANGUAGES.has(primary) ? "rtl" : "ltr";
}

/**
 * The locale to format numbers, money and dates with: the language in a country
 * (`de` + `CH` → `de-CH`, `sr-Latn` + `rs` → `sr-Latn-RS`). A tag that already names a
 * region keeps it; a tag that isn't one is returned as given.
 */
export function localeOf(language: string, country?: string): string {
  try {
    const locale = new Intl.Locale(language);
    if (locale.region !== undefined || country === undefined) return locale.toString();
    return new Intl.Locale(language, { region: country.toUpperCase() }).toString();
  } catch {
    return language;
  }
}

/** What a storefront URL's first segment names. */
export interface LocaleSegment {
  /** The language, when the segment names one: otherwise the market's default. */
  language?: string;
  /** The country, lower-cased as URLs have it. */
  country: string;
}

/**
 * Reads a storefront URL's locale segment: `de` is a country in its market's default
 * language, `en-de` and `sr-latn-rs` a language (`en`, `sr-Latn`) in a country, the
 * last part. Undefined for anything else.
 */
export function parseLocaleSegment(segment: string): LocaleSegment | undefined {
  const parts = segment.toLowerCase().split("-");
  const country = parts.pop();
  if (country === undefined || !/^[a-z]{2}$/.test(country)) return undefined;
  if (parts.length === 0) return { country };
  // A language tag starts with the language: two or three letters.
  if (!/^[a-z]{2,3}$/.test(parts[0] ?? "")) return undefined;
  const language = canonicalLanguage(parts.join("-"));
  if (language === undefined) return undefined;
  return { language, country };
}

/** The segment for a country, in a language when one is named: `parseLocaleSegment`'s inverse. */
export function formatLocaleSegment(locale: LocaleSegment): string {
  const country = locale.country.toLowerCase();
  return locale.language === undefined ? country : `${locale.language.toLowerCase()}-${country}`;
}
