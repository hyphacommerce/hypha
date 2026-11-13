// The store's content languages: the tags the admin offers, checking one someone typed,
// reordering a market's list and reading the API's "still in use" answer. Plain
// functions, on both sides and under `node --test`. Nothing here is about the language
// the admin itself speaks (`lib/language.ts`).

/** The most languages a store has beside its default one. */
export const MAX_LANGUAGES = 20;

/**
 * The tags offered by name; any other BCP 47 tag can be typed. Regional and script
 * variants are listed where shops commonly tell them apart.
 */
export const COMMON_LANGUAGES: readonly string[] = [
  "ar",
  "bg",
  "bs",
  "ca",
  "cs",
  "da",
  "de",
  "de-AT",
  "de-CH",
  "el",
  "en",
  "en-GB",
  "en-US",
  "es",
  "es-MX",
  "et",
  "fi",
  "fr",
  "fr-CA",
  "ga",
  "he",
  "hi",
  "hr",
  "hu",
  "id",
  "is",
  "it",
  "ja",
  "ko",
  "lt",
  "lv",
  "mk",
  "nb",
  "nl",
  "pl",
  "pt",
  "pt-BR",
  "ro",
  "ru",
  "sk",
  "sl",
  "sq",
  "sr-Cyrl",
  "sr-Latn",
  "sv",
  "th",
  "tr",
  "uk",
  "vi",
  "zh-Hans",
  "zh-Hant",
];

/** The API's rule for a tag: a language, then optionally a script, a region, variants. */
const TAG =
  /^[A-Za-z]{2,3}(-[A-Za-z]{4})?(-([A-Za-z]{2}|[0-9]{3}))?(-([A-Za-z0-9]{5,8}|[0-9][A-Za-z0-9]{3}))*$/;

/**
 * A typed tag in canonical case (`SR-latn` → `sr-Latn`), or `null` for what the API
 * would refuse. Only the case is changed: `Intl` would also rewrite deprecated subtags,
 * and the API keeps the tag it was given.
 */
export function canonicalTag(input: string): string | null {
  const tag = input.trim();
  if (tag.length > 35 || !TAG.test(tag)) return null;
  return tag
    .split("-")
    .map((part, index) => {
      if (index === 0) return part.toLowerCase();
      if (part.length === 4 && /^[A-Za-z]+$/.test(part))
        return `${part.charAt(0).toUpperCase()}${part.slice(1).toLowerCase()}`;
      if (part.length === 2) return part.toUpperCase();
      return part.toLowerCase();
    })
    .join("-");
}

/**
 * The tag without its region and what follows it (`de-AT` → `de`, `sr-Latn-RS` →
 * `sr-Latn`): the language a regional wording falls back to, as the API reads it.
 * `null` for a tag that names no region. Takes a canonical tag, as the API answers them.
 */
export function withoutRegion(tag: string): string | null {
  const subtags = tag.split("-");
  // In canonical case only a region is two capitals or three digits.
  const at = subtags.findIndex(
    (subtag, index) => index > 0 && /^([A-Z]{2}|[0-9]{3})$/.test(subtag),
  );
  return at === -1 ? null : subtags.slice(0, at).join("-");
}

/** Whether two tags name the same language, whatever their case. */
export function sameTag(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/**
 * `list` with the item at `index` moved `delta` places (−1 up, 1 down); the list as it
 * is when that would leave it.
 */
export function moved<T>(list: readonly T[], index: number, delta: number): T[] {
  const to = index + delta;
  if (index < 0 || index >= list.length || to < 0 || to >= list.length) return [...list];
  const next = [...list];
  next.splice(to, 0, ...next.splice(index, 1));
  return next;
}

/**
 * A language as lists and selects show it: its name in the reader's language with the
 * tag after it, "German (de)"; the tag alone when `Intl` has no name. A name that has
 * parentheses of its own ("Serbian (Latin)", "Chinese (Traditional)", or full-width ones
 * in Chinese and Japanese) gets the tag after a dot instead, "Serbian (Latin) · sr-Latn",
 * so the two never read as doubled brackets.
 */
export function languageLabel(format: { languageName(tag: string): string }, tag: string): string {
  const name = format.languageName(tag);
  if (name === tag) return tag;
  return /[(（]/.test(name) ? `${name} · ${tag}` : `${name} (${tag})`;
}

/**
 * A market's list with `tag` added at the end. An empty list stands for the store's
 * default language alone, so the first language added comes after that default rather
 * than replacing it: a market offering German besides English stays English first.
 */
export function addedLanguage(
  list: readonly string[],
  tag: string,
  storeDefault: string,
): string[] {
  if (list.includes(tag)) return [...list];
  if (list.length === 0 && tag !== storeDefault) return [storeDefault, tag];
  return [...list, tag];
}

/** A store's language as the API lists it. */
export type StoreLanguage = { language: string; published: boolean };

/**
 * What a market can offer: the store's default language, then its published ones. An
 * unpublished language is served to no shopper, so no market lists it.
 */
export function offeredLanguages(
  defaultLanguage: string,
  languages: readonly StoreLanguage[],
): string[] {
  return [defaultLanguage, ...languages.filter((l) => l.published).map((l) => l.language)];
}

/** A market that still offers a language, from a `language_in_use` answer. */
export type MarketInUse = { id: string; languages: string[] };

/**
 * The markets a `409 language_in_use` names (`details.markets`); empty when the answer
 * has none (a caller without `markets:read` isn't told).
 */
export function marketsInUse(details: unknown): MarketInUse[] {
  if (typeof details !== "object" || details === null) return [];
  const markets: unknown = Reflect.get(details, "markets");
  if (!Array.isArray(markets)) return [];
  return markets.flatMap((market: unknown): MarketInUse[] => {
    if (typeof market !== "object" || market === null) return [];
    const id: unknown = Reflect.get(market, "id");
    const languages: unknown = Reflect.get(market, "languages");
    if (typeof id !== "string") return [];
    return [
      {
        id,
        languages: Array.isArray(languages)
          ? languages.filter((l): l is string => typeof l === "string")
          : [],
      },
    ];
  });
}
