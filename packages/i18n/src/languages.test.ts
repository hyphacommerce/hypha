import assert from "node:assert/strict";
import { test } from "node:test";

import {
  canonicalLanguage,
  directionOf,
  formatLocaleSegment,
  localeOf,
  matchLanguage,
  parseAcceptLanguage,
  parseLocaleSegment,
} from "./languages.ts";

test("parseAcceptLanguage orders by q, then by position", () => {
  assert.deepEqual(parseAcceptLanguage("de-CH,de;q=0.9,en;q=0.8,fr;q=0.9"), [
    "de-CH",
    "de",
    "fr",
    "en",
  ]);
});

test("parseAcceptLanguage canonicalises tags and tolerates spaces", () => {
  assert.deepEqual(parseAcceptLanguage(" sr-latn-rs ; q=0.5 , EN-us "), ["en-US", "sr-Latn-RS"]);
});

test("parseAcceptLanguage drops *, q=0, repeats and what isn't a tag", () => {
  assert.deepEqual(parseAcceptLanguage("*, en;q=0, de, de;q=0.5, not a tag, fr;q=2, ,"), ["de"]);
});

test("parseAcceptLanguage of nothing is nothing", () => {
  assert.deepEqual(parseAcceptLanguage(null), []);
  assert.deepEqual(parseAcceptLanguage(undefined), []);
  assert.deepEqual(parseAcceptLanguage(""), []);
});

test("matchLanguage: the exact tag, in the available spelling", () => {
  assert.equal(matchLanguage(["sr-latn"], ["en", "sr-Latn"]), "sr-Latn");
  assert.equal(matchLanguage(["EN"], ["en"]), "en");
});

test("matchLanguage: the tag without its region", () => {
  assert.equal(matchLanguage(["de-CH"], ["en", "de"]), "de");
  assert.equal(matchLanguage(["sr-Latn-RS"], ["sr", "sr-Latn"]), "sr-Latn");
  assert.equal(matchLanguage(["sr-Latn-RS"], ["sr"]), "sr");
});

test("matchLanguage: an exact match beats the shorter tag", () => {
  assert.equal(matchLanguage(["de-CH"], ["de", "de-CH"]), "de-CH");
});

test("matchLanguage: wanted order decides, each tried fully before the next", () => {
  assert.equal(matchLanguage(["fr-CA", "de"], ["de", "fr"]), "fr");
  assert.equal(matchLanguage(["it", "de"], ["de", "fr"]), "de");
});

test("matchLanguage never widens a wanted language to a regional one", () => {
  assert.equal(matchLanguage(["de"], ["de-CH"]), undefined);
  assert.equal(matchLanguage(["sr"], ["sr-Latn"]), undefined);
});

test("matchLanguage: nothing wanted, nothing available, nothing in common", () => {
  assert.equal(matchLanguage([], ["en"]), undefined);
  assert.equal(matchLanguage(["en"], []), undefined);
  assert.equal(matchLanguage(["ja"], ["en", "de"]), undefined);
  assert.equal(matchLanguage([""], ["en"]), undefined);
});

test("matchLanguage drops an extension with its values", () => {
  assert.equal(matchLanguage(["de-u-co-phonebk"], ["de", "de-u"]), "de");
});

test("directionOf: right-to-left languages", () => {
  for (const language of ["ar", "he", "fa", "ur", "ps", "yi", "ar-EG", "he-IL", "FA-ir"]) {
    assert.equal(directionOf(language), "rtl", language);
  }
});

test("directionOf: everything else is left-to-right", () => {
  for (const language of ["en", "de-CH", "sr-Latn", "sr-Cyrl-RS", "ja", "zh-Hant", ""]) {
    assert.equal(directionOf(language), "ltr", language);
  }
});

test("directionOf: a script in the tag decides", () => {
  assert.equal(directionOf("az-Arab"), "rtl");
  assert.equal(directionOf("ur-Latn"), "ltr");
  assert.equal(directionOf("ks-Deva-IN"), "ltr");
});

test("localeOf adds the country as the region", () => {
  assert.equal(localeOf("de", "CH"), "de-CH");
  assert.equal(localeOf("de", "ch"), "de-CH");
  assert.equal(localeOf("sr-Latn", "rs"), "sr-Latn-RS");
});

test("localeOf keeps a region the tag already has", () => {
  assert.equal(localeOf("en-GB", "DE"), "en-GB");
  assert.equal(localeOf("sr-Latn-ME", "RS"), "sr-Latn-ME");
});

test("localeOf without a country is the language", () => {
  assert.equal(localeOf("de"), "de");
  assert.equal(localeOf("sr-latn"), "sr-Latn");
});

test("localeOf returns what isn't a tag, or has no usable country, as given", () => {
  assert.equal(localeOf("not a tag", "DE"), "not a tag");
  assert.equal(localeOf("de", "Germany"), "de");
});

test("parseLocaleSegment: a country alone", () => {
  assert.deepEqual(parseLocaleSegment("de"), { country: "de" });
  assert.deepEqual(parseLocaleSegment("DE"), { country: "de" });
});

test("parseLocaleSegment: a language and a country, the last part", () => {
  assert.deepEqual(parseLocaleSegment("en-de"), { language: "en", country: "de" });
  assert.deepEqual(parseLocaleSegment("sr-latn-rs"), { language: "sr-Latn", country: "rs" });
  assert.deepEqual(parseLocaleSegment("de-ch"), { language: "de", country: "ch" });
  assert.deepEqual(parseLocaleSegment("en-gb-de"), { language: "en-GB", country: "de" });
});

test("parseLocaleSegment refuses anything else", () => {
  for (const segment of ["", "d", "deu", "products", "sr-latn", "en-", "-de", "en--de", "1-de"]) {
    assert.equal(parseLocaleSegment(segment), undefined, segment);
  }
});

test("locale segments round trip", () => {
  for (const segment of ["de", "en-de", "sr-latn-rs", "zh-hant-tw", "pt-br"]) {
    const parsed = parseLocaleSegment(segment);
    assert.ok(parsed, segment);
    assert.equal(formatLocaleSegment(parsed), segment);
  }
  for (const locale of [
    { country: "rs" },
    { language: "sr-Latn", country: "rs" },
    { language: "en", country: "de" },
  ]) {
    assert.deepEqual(parseLocaleSegment(formatLocaleSegment(locale)), locale);
  }
});

test("formatLocaleSegment lower-cases", () => {
  assert.equal(formatLocaleSegment({ language: "sr-Latn", country: "RS" }), "sr-latn-rs");
  assert.equal(formatLocaleSegment({ country: "DE" }), "de");
});

test("canonicalLanguage spells a tag as BCP 47 does, or refuses it", () => {
  assert.equal(canonicalLanguage("sr-latn-rs"), "sr-Latn-RS");
  assert.equal(canonicalLanguage("EN"), "en");
  assert.equal(canonicalLanguage("not a tag"), undefined);
  assert.equal(canonicalLanguage(""), undefined);
});
