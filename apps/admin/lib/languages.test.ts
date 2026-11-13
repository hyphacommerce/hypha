import assert from "node:assert/strict";
import { test } from "node:test";

import {
  COMMON_LANGUAGES,
  addedLanguage,
  canonicalTag,
  languageLabel,
  languagesInUse,
  marketsInUse,
  moved,
  offeredLanguages,
  sameTag,
  withoutRegion,
} from "./languages.ts";
import { createFormat } from "./format.ts";
import { formatLocale } from "./pick-language.ts";

test("a typed tag is put in canonical case", () => {
  assert.equal(canonicalTag("de"), "de");
  assert.equal(canonicalTag(" EN-gb "), "en-GB");
  assert.equal(canonicalTag("SR-latn"), "sr-Latn");
  assert.equal(canonicalTag("sr-latn-rs"), "sr-Latn-RS");
  assert.equal(canonicalTag("es-419"), "es-419");
  assert.equal(canonicalTag("de-CH-1901"), "de-CH-1901");
  assert.equal(canonicalTag("sl-ROZAJ"), "sl-rozaj");
});

test("what isn't a tag is refused", () => {
  assert.equal(canonicalTag(""), null);
  assert.equal(canonicalTag("german"), null);
  assert.equal(canonicalTag("en_GB"), null);
  assert.equal(canonicalTag("e"), null);
  assert.equal(canonicalTag("en-"), null);
  assert.equal(canonicalTag(`en-${"abcdefgh-".repeat(4)}abcde`), null);
});

test("every offered tag is canonical and listed once", () => {
  for (const tag of COMMON_LANGUAGES) assert.equal(canonicalTag(tag), tag);
  assert.equal(new Set(COMMON_LANGUAGES).size, COMMON_LANGUAGES.length);
});

test("a regional tag falls back to the tag without its region", () => {
  assert.equal(withoutRegion("de-AT"), "de");
  assert.equal(withoutRegion("sr-Latn-RS"), "sr-Latn");
  assert.equal(withoutRegion("es-419"), "es");
  assert.equal(withoutRegion("de-CH-1901"), "de");
});

test("a tag without a region has nothing to fall back to", () => {
  assert.equal(withoutRegion("de"), null);
  assert.equal(withoutRegion("sr-Latn"), null);
  assert.equal(withoutRegion("sl-rozaj"), null);
});

test("tags are compared whatever their case", () => {
  assert.equal(sameTag("sr-Latn", "SR-LATN"), true);
  assert.equal(sameTag("de", "de-AT"), false);
});

test("an item moves one place up or down", () => {
  assert.deepEqual(moved(["de", "en", "fr"], 1, -1), ["en", "de", "fr"]);
  assert.deepEqual(moved(["de", "en", "fr"], 1, 1), ["de", "fr", "en"]);
});

test("a move off either end leaves the list as it is", () => {
  assert.deepEqual(moved(["de", "en"], 0, -1), ["de", "en"]);
  assert.deepEqual(moved(["de", "en"], 1, 1), ["de", "en"]);
  assert.deepEqual(moved(["de", "en"], 5, -1), ["de", "en"]);
  assert.deepEqual(moved([], 0, 1), []);
});

test("moving answers a new list", () => {
  const list = ["de", "en"];
  assert.notEqual(moved(list, 0, 1), list);
  assert.deepEqual(list, ["de", "en"]);
});

test("the first language added to an empty list comes after the store's default", () => {
  // Empty means the store's default alone: adding German keeps English, first.
  assert.deepEqual(addedLanguage([], "de", "en"), ["en", "de"]);
  assert.deepEqual(addedLanguage([], "en", "en"), ["en"]);
});

test("a language added to a list goes at its end, once", () => {
  assert.deepEqual(addedLanguage(["de"], "fr", "en"), ["de", "fr"]);
  assert.deepEqual(addedLanguage(["de", "en"], "de", "en"), ["de", "en"]);
});

test("a market is offered the default language and the published ones", () => {
  assert.deepEqual(
    offeredLanguages("en", [
      { language: "de", published: true },
      { language: "fr", published: false },
      { language: "sr-Latn", published: true },
    ]),
    ["en", "de", "sr-Latn"],
  );
  assert.deepEqual(offeredLanguages("en", []), ["en"]);
});

test("the markets still offering a language are read from the answer's details", () => {
  assert.deepEqual(
    marketsInUse({ markets: [{ id: "mkt_1", languages: ["de"] }, { id: "mkt_2" }, { name: "x" }] }),
    [
      { id: "mkt_1", languages: ["de"] },
      { id: "mkt_2", languages: [] },
    ],
  );
});

test("an answer naming no markets gives none", () => {
  assert.deepEqual(marketsInUse(null), []);
  assert.deepEqual(marketsInUse({}), []);
  assert.deepEqual(marketsInUse({ markets: "mkt_1" }), []);
});

const markets = [
  { id: "mkt_eu", name: "Europe", languages: ["en", "de"] },
  { id: "mkt_at", name: "Austria", languages: ["de"] },
  { id: "mkt_us", name: "US", languages: [] },
];

test("removing a language a market offers is in that market's way", () => {
  assert.deepEqual(languagesInUse(["en", "de"], ["en"], markets), [
    { id: "mkt_eu", name: "Europe", languages: ["de"] },
    { id: "mkt_at", name: "Austria", languages: ["de"] },
  ]);
});

test("unpublishing is in the way like removing", () => {
  const after = offeredLanguages("en", [{ language: "de", published: false }]);
  assert.deepEqual(
    languagesInUse(["en", "de"], after, markets).map((m) => m.id),
    ["mkt_eu", "mkt_at"],
  );
});

test("a language the store didn't offer before blocks nothing", () => {
  // A market already naming an unpublished language doesn't stop its removal.
  assert.deepEqual(languagesInUse(["en"], ["en"], markets), []);
});

test("a language no market names is removed freely", () => {
  assert.deepEqual(languagesInUse(["en", "de", "fr"], ["en", "de"], markets), []);
});

test("languages in the way are matched whatever their case", () => {
  assert.deepEqual(
    languagesInUse(["en", "sr-Latn"], ["en"], [{ id: "m", languages: ["SR-latn"] }]),
    [{ id: "m", languages: ["SR-latn"] }],
  );
});

const names: Record<string, string> = {
  de: "German",
  "sr-Latn": "Serbian (Latin)",
  "zh-Hant": "中文（繁體）",
};
const named = { languageName: (tag: string) => names[tag] ?? tag };

test("a language is labelled by its name with the tag in parentheses", () => {
  assert.equal(languageLabel(named, "de"), "German (de)");
});

test("a name with parentheses of its own gets the tag after a dot", () => {
  assert.equal(languageLabel(named, "sr-Latn"), "Serbian (Latin) · sr-Latn");
  assert.equal(languageLabel(named, "zh-Hant"), "中文（繁體） · zh-Hant");
});

test("a language without a name is labelled by its tag alone", () => {
  assert.equal(languageLabel(named, "x-unknown"), "x-unknown");
});

test("Intl's own name for Serbian (Latin) isn't bracketed twice", () => {
  const format = createFormat(formatLocale("en"));
  assert.equal(languageLabel(format, "sr-Latn"), "Serbian (Latin) · sr-Latn");
});
