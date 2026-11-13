// How URLs carry the shopper's country and language. Shared by the proxy, server pages
// and client components: it reads nothing but its arguments.
//
// With folders (the default), a URL's first segment is its locale: `/de/…` is Germany
// in its market's default language, `/en-de/…` Germany in English (the last part is the
// country). In a single tree (`STOREFRONT_ROUTING=single`) the country stays out of
// URLs: `/…` is the default language, `/en/…` another, and the proxy rewrites both to
// the folder form, so the `[country]` segment pages read is a locale segment either
// way.

import {
  canonicalLanguage,
  formatLocaleSegment,
  localeOf,
  matchLanguage,
  parseAcceptLanguage,
  parseLocaleSegment,
} from "@hyphacommerce/i18n";

/** The cookie holding the shopper's country code (`HR`). */
export const COUNTRY_COOKIE = "country";

/** The cookie holding the language the shopper picked (`sr-Latn`, the API's spelling). */
export const LANGUAGE_COOKIE = "language";

export type Routing = "folders" | "single";

/** `STOREFRONT_ROUTING`: `single` keeps the country out of URLs; anything else is folders. */
export function routing(value: string | undefined): Routing {
  return value === "single" ? "single" : "folders";
}

/** A country and the languages it's offered in (`/storefront/countries`): the first is
 * its default. */
export type Offer = { country: string; languages: readonly string[] };

/** Where a page is: the country, the language to ask the API in (one of the country's
 * own) and the locale segment that names both. */
export type Locale = { country: string; language: string; segment: string };

/** A language the API always has an answer for, when a country lists none (it never
 * does: the API fills in the store's default). */
const FALLBACK_LANGUAGE = "en";

/** A country's default language: its first. */
export function defaultLanguage(offer: Offer): string {
  return offer.languages[0] ?? FALLBACK_LANGUAGE;
}

/** The country code a locale segment names (`hr` and `en-hr` → `HR`); empty for a
 * segment that names none, so nothing else in it can reach a link. */
export function segmentCountry(segment: string): string {
  return parseLocaleSegment(segment)?.country.toUpperCase() ?? "";
}

/**
 * The country's language a wanted one comes to, in the country's spelling: itself, the
 * same without its region (`de-AT` → `de`), or the same language wider (`en` →
 * `en-GB`); `undefined` when the country offers none of them.
 *
 * Broader than the API on purpose: the API never widens (the exact tag, then the tag
 * without its region, then the market's default), but a URL (`/en-rs`, a cookie, an
 * `Accept-Language` of `en`) is better sent to the country's English than to its
 * default. Nothing is served wrongly by it: what this answers is always one of the
 * country's own tags, so the API is only ever asked in a language it offers.
 */
export function offered(wanted: string | undefined, offer: Offer): string | undefined {
  if (wanted === undefined) return undefined;
  // Compared as BCP 47 spells them, since a segment is read that way (`iw` is `he`),
  // and answered in the country's own spelling, the API's.
  const spelled = new Map<string, string>();
  for (const tag of offer.languages) {
    const canonical = canonicalLanguage(tag) ?? tag;
    if (!spelled.has(canonical)) spelled.set(canonical, tag);
  }
  const found = matchLanguage([canonicalLanguage(wanted) ?? wanted], [...spelled.keys()]);
  return found === undefined ? undefined : spelled.get(found);
}

/** The language of a locale segment in its country: the one it names when the country
 * offers it (or one it comes to, `offered`), else the country's default. */
export function segmentLanguage(segment: string, offer: Offer): string {
  return offered(parseLocaleSegment(segment)?.language, offer) ?? defaultLanguage(offer);
}

/** The segment for `language` in a country: the country alone for its default (and
 * for no language), `en-de` for another. */
export function localeSegment(offer: Offer, language?: string): string {
  const country = offer.country.toLowerCase();
  if (language === undefined || language.toLowerCase() === defaultLanguage(offer).toLowerCase()) {
    return country;
  }
  return formatLocaleSegment({ language, country });
}

/**
 * Where a page about something written in `language` (an order) belongs in the page's
 * country: that language's segment there (or the one it comes to, `offered`).
 * `undefined` when the page is in it already (`page` is the page's segment), or when
 * the country offers nothing it comes to (an order from before languages, or in one the
 * country dropped since): the page stays where it is.
 */
export function segmentFor(
  language: string | null | undefined,
  offer: Offer,
  page: string,
): string | undefined {
  const there = offered(language ?? undefined, offer);
  if (there === undefined) return undefined;
  const segment = localeSegment(offer, there);
  return segment === page.toLowerCase() ? undefined : segment;
}

/** The one segment a page in a segment's country and language lives at: `de-de` and
 * `fr-de` (not offered) are `de`, `en-de` stays, `en-gb-de` is `en-de` where the
 * country offers plain `en`. */
export function canonicalSegment(segment: string, offer: Offer): string {
  return localeSegment(offer, segmentLanguage(segment, offer));
}

/** The place a locale segment names, among `countries`; `undefined` for a country none
 * sells to. */
export function localeOfSegment(segment: string, countries: readonly Offer[]): Locale | undefined {
  const country = segmentCountry(segment);
  const offer = countries.find((c) => c.country === country);
  if (!offer) return undefined;
  const language = segmentLanguage(segment, offer);
  return { country, language, segment: localeSegment(offer, language) };
}

/** Every place the store sells in: each country in each of its languages, the
 * country's default first. */
export function locales(countries: readonly Offer[]): Locale[] {
  return countries.flatMap((offer) =>
    (offer.languages.length > 0 ? offer.languages : [defaultLanguage(offer)]).map((language) => ({
      country: offer.country,
      language,
      segment: localeSegment(offer, language),
    })),
  );
}

/**
 * The places a page has a URL of its own in, for `hreflang` and the sitemap, each with
 * its tag (`de-CH`: the language in the country; a language that names its region
 * keeps it). With folders every country in each of its languages; a single tree has one
 * URL per language, its country the shopper's, so it lists the first country's
 * languages (its default unprefixed), tagged by the language alone.
 */
export function alternatePlaces(
  mode: Routing,
  countries: readonly Offer[],
): { tag: string; segment: string }[] {
  return tagged(mode, countries).map(({ tag, segment }) => ({ tag, segment }));
}

/** `alternatePlaces` with each place's locale beside its tag. */
function tagged(mode: Routing, countries: readonly Offer[]): (Locale & { tag: string })[] {
  if (mode === "single") {
    const [first] = countries;
    return first ? locales([first]).map((l) => ({ ...l, tag: l.language })) : [];
  }
  return locales(countries).map((l) => ({ ...l, tag: localeOf(l.language, l.country) }));
}

/** The region a language tag names (`AT` in `de-AT`, none in `sr-Latn`). */
function regionOf(tag: string): string | undefined {
  try {
    return new Intl.Locale(tag).region;
  } catch {
    return undefined;
  }
}

/**
 * The places to name as `hreflang` alternates: `alternatePlaces`, each tag once, except
 * in a single tree whose countries' default languages differ, where none. There the
 * unprefixed URL is each shopper's country's default language, so no one tag is true of
 * it.
 *
 * Two places can come to one tag: `de` and `de-AT` offered in Austria are both `de-AT`
 * (and so is `de-AT` offered in Germany). A tag names one URL, so one of them keeps it:
 * the page in the tag's own country, then the one whose language names its region (the
 * merchant offered `de-AT` for it on purpose), then the first listed. The others are in
 * no `hreflang` set, not even their own (they name none): they stay in the sitemap, with
 * their canonical pointing at themselves like any page's.
 */
export function hreflangPlaces(
  mode: Routing,
  countries: readonly Offer[],
): { tag: string; segment: string }[] {
  if (mode === "single" && new Set(countries.map(defaultLanguage)).size > 1) return [];
  const places = tagged(mode, countries);
  const claim = (place: Locale & { tag: string }) =>
    (regionOf(place.tag) === place.country ? 2 : 0) + (regionOf(place.language) ? 1 : 0);
  const kept = new Map<string, Locale & { tag: string }>();
  for (const place of places) {
    const key = place.tag.toLowerCase();
    const held = kept.get(key);
    if (held === undefined || claim(place) > claim(held)) kept.set(key, place);
  }
  return places
    .filter((place) => kept.get(place.tag.toLowerCase()) === place)
    .map(({ tag, segment }) => ({ tag, segment }));
}

/**
 * Every prefix a page's path may have here: each country in each of its languages with
 * folders (`/de`, `/en-de`), the default's none and each other language's in a single
 * tree (``, `/en`), for robots.
 */
export function prefixes(mode: Routing, countries: readonly Offer[]): string[] {
  return [...new Set(locales(countries).map((l) => href(mode, l.segment, "")))];
}

/**
 * A link to `path` (`/p/mug`, `/`) in a place, named by its locale segment (`en-hr`) or
 * by a country alone (`HR`: its default language). With folders that's
 * `/en-hr/p/mug`; in a single tree `/en/p/mug`, or the path itself for the default
 * language, where the proxy adds the country back.
 */
export function href(mode: Routing, segment: string, path: string): string {
  const prefix =
    mode === "single"
      ? (parseLocaleSegment(segment)?.language?.toLowerCase() ?? "")
      : segment.toLowerCase();
  if (prefix === "") return path;
  // No trailing slash: Next.js redirects `/hr/` to `/hr` anyway.
  return path === "/" || path === "" ? `/${prefix}` : `/${prefix}${path}`;
}

/** The country to use, from the cookie, then the host's geolocation, then the first. */
export function pickCountry(
  available: string[],
  cookie: string | undefined,
  geolocated: string | undefined | null,
): string | undefined {
  for (const candidate of [cookie, geolocated]) {
    const code = candidate?.toUpperCase();
    if (code && available.includes(code)) return code;
  }
  return available[0];
}

/**
 * The language for a shopper whose URL names none: the one they picked (the `language`
 * cookie), then what their browser asks for (`Accept-Language`), then the country's
 * default. Always one of the country's own.
 */
export function pickLanguage(
  offer: Offer,
  cookie: string | undefined,
  acceptLanguage: string | undefined | null,
): string {
  const picked = cookie === undefined ? undefined : canonicalLanguage(cookie);
  return (
    offered(picked, offer) ??
    matchLanguage(parseAcceptLanguage(acceptLanguage), offer.languages) ??
    defaultLanguage(offer)
  );
}

/** A language tag as a URL part: a two or three letter language, then its other parts. */
const LANGUAGE_PART = /^[a-z]{2,3}(-[a-z0-9]{1,8})*$/i;

/**
 * A single tree's path, split into the language its first part names and the rest
 * (`/en/p/mug` → `en`, `/p/mug`). A first part is a language only when some country
 * offers it (or one it comes to): no page's own path starts with a language tag, but
 * `/search` and `/order` would read as ones by their shape alone.
 */
export function splitLanguage(
  pathname: string,
  countries: readonly Offer[],
): { language?: string; rest: string } {
  const first = pathname.split("/")[1] ?? "";
  const tag = LANGUAGE_PART.test(first) ? canonicalLanguage(first) : undefined;
  if (tag === undefined || !countries.some((offer) => offered(tag, offer) !== undefined)) {
    return { rest: pathname === "" ? "/" : pathname };
  }
  return { language: tag, rest: pathname.slice(first.length + 1) || "/" };
}

/**
 * A page's path without its locale: `/en-de/p/mug` → `/p/mug` with folders, `/en/p/mug`
 * → `/p/mug` in a single tree. A query string stays.
 */
export function withoutLocale(mode: Routing, path: string, countries: readonly Offer[]): string {
  const cut = path.search(/[?#]/);
  const pathname = cut === -1 ? path : path.slice(0, cut);
  const tail = cut === -1 ? "" : path.slice(cut);
  if (mode === "single") return splitLanguage(pathname, countries).rest + tail;
  const first = pathname.split("/")[1] ?? "";
  const names = parseLocaleSegment(first);
  const known = names && countries.some((c) => c.country === names.country.toUpperCase());
  return (known ? pathname.slice(first.length + 1) || "/" : pathname || "/") + tail;
}

/** What the proxy does with a page request. */
export type Route =
  | { kind: "next" }
  /** To another URL: `path` has no query (the caller keeps the request's). */
  | { kind: "redirect"; path: string }
  /** Rendered from the folder form, the URL unchanged. */
  | { kind: "rewrite"; path: string };

const join = (prefix: string, rest: string) => (rest === "/" ? `/${prefix}` : `/${prefix}${rest}`);

/**
 * Where a page request goes.
 *
 * With folders: a path under a country the store sells to is served as it is, unless
 * it names a language that isn't the one its URL should (`/de-de`, a language the
 * country doesn't offer, `en-gb` where it offers `en`): that's redirected to the URL of
 * the language it comes to, the country's default when to none. Any other path is bare:
 * it's redirected into the shopper's country, in the language `pickLanguage` finds.
 *
 * In a single tree every path is rewritten to the shopper's country's folder: `/en/…`
 * in English when the country offers it (else redirected like a folder's: to the
 * language it comes to, or to the path without a prefix), a path without a language in
 * the country's default, which is that language's URL: a single tree has no bare paths,
 * so the cookie and `Accept-Language` decide nothing there.
 *
 * A URL that names a language is never redirected because of a cookie or a header.
 */
export function routeFor(input: {
  mode: Routing;
  pathname: string;
  countries: readonly Offer[];
  countryCookie?: string;
  geolocated?: string | null;
  languageCookie?: string;
  acceptLanguage?: string | null;
}): Route {
  const { mode, pathname, countries } = input;
  const code = pickCountry(
    countries.map((c) => c.country),
    input.countryCookie,
    input.geolocated,
  );
  const shopper = countries.find((c) => c.country === code);
  // No market sells anywhere yet: the pages say so.
  if (!shopper) return { kind: "next" };
  const chosen = () => pickLanguage(shopper, input.languageCookie, input.acceptLanguage);

  if (mode === "single") {
    const { language, rest } = splitLanguage(pathname, countries);
    // No prefix is the default language's URL, not a bare one: served as it is.
    if (language === undefined) {
      return { kind: "rewrite", path: join(localeSegment(shopper), pathname) };
    }
    const segment = localeSegment(shopper, offered(language, shopper));
    const prefix = parseLocaleSegment(segment)?.language?.toLowerCase();
    if (prefix === undefined) return { kind: "redirect", path: rest };
    if (prefix !== pathname.split("/")[1]) return { kind: "redirect", path: join(prefix, rest) };
    return { kind: "rewrite", path: join(segment, rest) };
  }

  const first = pathname.split("/")[1] ?? "";
  const names = parseLocaleSegment(first);
  const named = names && countries.find((c) => c.country === names.country.toUpperCase());
  if (named) {
    // A country alone is served in any case (`/DE`), as before languages.
    if (names.language === undefined) return { kind: "next" };
    const segment = canonicalSegment(first, named);
    if (segment === first) return { kind: "next" };
    return { kind: "redirect", path: join(segment, pathname.slice(first.length + 1) || "/") };
  }
  return { kind: "redirect", path: join(localeSegment(shopper, chosen()), pathname || "/") };
}
