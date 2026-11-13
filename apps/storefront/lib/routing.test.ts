import assert from "node:assert/strict";
import { test } from "node:test";

import {
  alternatePlaces,
  canonicalSegment,
  href,
  localeOfSegment,
  pickLanguage,
  routeFor,
  segmentCountry,
  segmentLanguage,
  withoutLocale,
} from "./routing.ts";

const DE = { country: "DE", languages: ["de", "en"] };
const AT = { country: "AT", languages: ["de"] };
const RS = { country: "RS", languages: ["sr-Latn", "en-GB"] };
const COUNTRIES = [DE, AT, RS];

test("a segment names its country, the last part", () => {
  assert.equal(segmentCountry("de"), "DE");
  assert.equal(segmentCountry("en-de"), "DE");
  assert.equal(segmentCountry("sr-latn-rs"), "RS");
  assert.equal(segmentCountry("DE"), "DE");
});

test("a segment that names no country names none, so it never reaches a link", () => {
  assert.equal(segmentCountry("/evil"), "");
  assert.equal(segmentCountry("search"), "");
  assert.equal(segmentCountry(""), "");
});

test("a country alone is its default language", () => {
  assert.equal(segmentLanguage("de", DE), "de");
  assert.equal(segmentLanguage("rs", RS), "sr-Latn");
});

test("a segment's language is the country's, in the API's spelling", () => {
  assert.equal(segmentLanguage("en-de", DE), "en");
  assert.equal(segmentLanguage("sr-latn-rs", RS), "sr-Latn");
});

test("a regional tag comes to the country's language without the region", () => {
  assert.equal(segmentLanguage("en-gb-de", DE), "en");
});

test("a language comes to the country's wider one of the same language", () => {
  assert.equal(segmentLanguage("en-rs", RS), "en-GB");
});

test("a language the country doesn't offer is its default", () => {
  assert.equal(segmentLanguage("fr-de", DE), "de");
  assert.equal(segmentLanguage("en-at", AT), "de");
});

test("each page has one segment: the default spelled out, aliases and others go", () => {
  assert.equal(canonicalSegment("de", DE), "de");
  assert.equal(canonicalSegment("en-de", DE), "en-de");
  assert.equal(canonicalSegment("de-de", DE), "de");
  assert.equal(canonicalSegment("fr-de", DE), "de");
  assert.equal(canonicalSegment("en-gb-de", DE), "en-de");
  assert.equal(canonicalSegment("en-rs", RS), "en-gb-rs");
  assert.equal(canonicalSegment("sr-latn-rs", RS), "rs");
});

test("a segment's place, among the store's countries", () => {
  assert.deepEqual(localeOfSegment("en-de", COUNTRIES), {
    country: "DE",
    language: "en",
    segment: "en-de",
  });
  assert.equal(localeOfSegment("fr", COUNTRIES), undefined);
});

test("with folders, a link keeps the page's segment", () => {
  assert.equal(href("folders", "en-de", "/p/mug"), "/en-de/p/mug");
  assert.equal(href("folders", "de", "/"), "/de");
  assert.equal(href("folders", "DE", "/c/kits"), "/de/c/kits");
  assert.equal(href("folders", "sr-latn-rs", "/search?q=x"), "/sr-latn-rs/search?q=x");
});

test("in a single tree, a link keeps only another language", () => {
  assert.equal(href("single", "de", "/p/mug"), "/p/mug");
  assert.equal(href("single", "en-de", "/p/mug"), "/en/p/mug");
  assert.equal(href("single", "en-de", "/"), "/en");
  assert.equal(href("single", "en-gb-rs", "/cart"), "/en-gb/cart");
});

test("a page's path without its place, in both modes, its query kept", () => {
  assert.equal(withoutLocale("folders", "/en-de/p/mug?x=1", COUNTRIES), "/p/mug?x=1");
  assert.equal(withoutLocale("folders", "/de", COUNTRIES), "/");
  assert.equal(withoutLocale("single", "/en/c/kits?filter=a", COUNTRIES), "/c/kits?filter=a");
  assert.equal(withoutLocale("single", "/c/kits", COUNTRIES), "/c/kits");
  assert.equal(withoutLocale("single", "/en", COUNTRIES), "/");
});

test("the picked language wins, then the browser's, then the default", () => {
  assert.equal(pickLanguage(DE, "en", "de-DE,de"), "en");
  assert.equal(pickLanguage(DE, undefined, "en-US,en;q=0.9,de;q=0.5"), "en");
  assert.equal(pickLanguage(DE, undefined, "fr-FR,fr"), "de");
  assert.equal(pickLanguage(DE, "fr", undefined), "de");
  assert.equal(pickLanguage(RS, undefined, "sr-RS,sr;q=0.9"), "sr-Latn");
  assert.equal(pickLanguage(RS, undefined, "en-US"), "en-GB");
});

const route = (input: Partial<Parameters<typeof routeFor>[0]>) =>
  routeFor({ mode: "folders", pathname: "/", countries: COUNTRIES, ...input });

test("a bare path goes to the shopper's country in the language they'd read", () => {
  assert.deepEqual(route({ pathname: "/p/mug", countryCookie: "DE" }), {
    kind: "redirect",
    path: "/de/p/mug",
  });
  assert.deepEqual(route({ pathname: "/p/mug", countryCookie: "DE", acceptLanguage: "en-US,en" }), {
    kind: "redirect",
    path: "/en-de/p/mug",
  });
  assert.deepEqual(
    route({ pathname: "/", geolocated: "DE", languageCookie: "de", acceptLanguage: "en" }),
    { kind: "redirect", path: "/de" },
  );
});

test("a place's URL is served, whatever the cookie or the browser says", () => {
  const asks = { languageCookie: "en", acceptLanguage: "en", countryCookie: "AT" };
  assert.deepEqual(route({ pathname: "/de/p/mug", ...asks }), { kind: "next" });
  assert.deepEqual(route({ pathname: "/en-de", languageCookie: "de" }), { kind: "next" });
});

test("a place naming a language its country doesn't offer goes to the one it comes to", () => {
  assert.deepEqual(route({ pathname: "/fr-de/p/mug" }), { kind: "redirect", path: "/de/p/mug" });
  assert.deepEqual(route({ pathname: "/de-de" }), { kind: "redirect", path: "/de" });
  assert.deepEqual(route({ pathname: "/en-gb-de/c/x" }), {
    kind: "redirect",
    path: "/en-de/c/x",
  });
});

const single = (input: Partial<Parameters<typeof routeFor>[0]>) =>
  route({ mode: "single", countryCookie: "DE", ...input });

test("in a single tree, paths are rewritten to the shopper's place", () => {
  assert.deepEqual(single({ pathname: "/p/mug" }), { kind: "rewrite", path: "/de/p/mug" });
  assert.deepEqual(single({ pathname: "/" }), { kind: "rewrite", path: "/de" });
  assert.deepEqual(single({ pathname: "/en/p/mug" }), { kind: "rewrite", path: "/en-de/p/mug" });
  assert.deepEqual(single({ pathname: "/en" }), { kind: "rewrite", path: "/en-de" });
  // Unprefixed is the default language's URL: no header moves it.
  assert.deepEqual(single({ pathname: "/p/mug", acceptLanguage: "en", languageCookie: "en" }), {
    kind: "rewrite",
    path: "/de/p/mug",
  });
});

test("in a single tree, a language the country lacks goes to the one it comes to", () => {
  // The default spelled out: unprefixed.
  assert.deepEqual(single({ pathname: "/de/p/mug", countryCookie: "DE" }), {
    kind: "redirect",
    path: "/p/mug",
  });
  // English isn't offered in Austria.
  assert.deepEqual(single({ pathname: "/en/c/x", countryCookie: "AT" }), {
    kind: "redirect",
    path: "/c/x",
  });
  // Serbia offers `en-GB`.
  assert.deepEqual(single({ pathname: "/en/c/x", countryCookie: "RS" }), {
    kind: "redirect",
    path: "/en-gb/c/x",
  });
});

test("no country sold anywhere: the pages answer", () => {
  assert.deepEqual(route({ countries: [], pathname: "/p/x" }), { kind: "next" });
});

test("hreflang lists every country in each language, the first as the default", () => {
  assert.deepEqual(alternatePlaces("folders", COUNTRIES), [
    { tag: "de-DE", segment: "de" },
    { tag: "en-DE", segment: "en-de" },
    { tag: "de-AT", segment: "at" },
    { tag: "sr-Latn-RS", segment: "rs" },
    { tag: "en-GB", segment: "en-gb-rs" },
  ]);
});

test("a single tree's hreflang is the first country's languages", () => {
  assert.deepEqual(alternatePlaces("single", COUNTRIES), [
    { tag: "de", segment: "de" },
    { tag: "en", segment: "en-de" },
  ]);
});
