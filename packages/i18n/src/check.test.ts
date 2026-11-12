import assert from "node:assert/strict";
import { test } from "node:test";

import { checkCatalogs, placeholdersOf } from "./check.ts";

const en = {
  "cart.title": "Your cart",
  "cart.count": "{count, plural, one {# item} other {# items}}",
  "cart.terms": "Accept the <link>terms</link>, {name}.",
};

test("placeholdersOf reads every branch, sorted", () => {
  assert.deepEqual(
    placeholdersOf(
      "{count, plural, one {{name} has # item} other {<b>{name}</b> has # items on {day, date}}}",
    ),
    { arguments: ["count", "day", "name"], tags: ["b"] },
  );
  assert.deepEqual(placeholdersOf("Plain"), { arguments: [], tags: [] });
});

test("placeholdersOf throws on what isn't ICU MessageFormat", () => {
  assert.throws(() => placeholdersOf("{count, plural, one {# item}"));
});

test("complete, matching catalogs pass", () => {
  const de = {
    "cart.title": "Ihr Warenkorb",
    "cart.count": "{count, plural, one {# Artikel} other {# Artikel}}",
    "cart.terms": "{name}, akzeptiere die <link>Bedingungen</link>.",
  };
  assert.deepEqual(checkCatalogs({ en, de }), { source: "en", problems: [], missing: { de: [] } });
});

test("what a language lacks is reported, not a problem", () => {
  const report = checkCatalogs({ en, de: { "cart.title": "Ihr Warenkorb" } });
  assert.deepEqual(report.problems, []);
  assert.deepEqual(report.missing, { de: ["cart.count", "cart.terms"] });
});

test("a language that must be complete fails for each key it lacks", () => {
  const report = checkCatalogs(
    { en, de: { "cart.title": "Ihr Warenkorb" }, fr: {} },
    { complete: ["de"] },
  );
  assert.deepEqual(report.problems, [
    { code: "missing_key", language: "de", key: "cart.count", message: "must be translated" },
    { code: "missing_key", language: "de", key: "cart.terms", message: "must be translated" },
  ]);
  assert.equal(report.missing.fr?.length, 3);
});

test("a key the source lacks is a problem", () => {
  const report = checkCatalogs({ en, de: { "cart.titel": "Ihr Warenkorb" } });
  assert.deepEqual(report.problems, [
    { code: "unknown_key", language: "de", key: "cart.titel", message: "en has no such key" },
  ]);
});

test("placeholders and tags must be the source's", () => {
  const report = checkCatalogs({
    en,
    de: {
      "cart.count": "{anzahl} Artikel",
      "cart.terms": "Akzeptiere die <b>Bedingungen</b>, {name}.",
    },
  });
  assert.deepEqual(report.problems, [
    {
      code: "placeholder_mismatch",
      language: "de",
      key: "cart.count",
      message: "lacks placeholder count; has placeholder anzahl the source doesn't",
    },
    {
      code: "placeholder_mismatch",
      language: "de",
      key: "cart.terms",
      message: "lacks tag link; has tag b the source doesn't",
    },
  ]);
});

test("a language may format a placeholder its own way", () => {
  // Japanese has one plural form: no `plural` needed for the same `count`.
  const report = checkCatalogs({ en, ja: { "cart.count": "{count} 点" } });
  assert.deepEqual(report.problems, []);
});

test("every message must parse, in every language", () => {
  const report = checkCatalogs({
    en: { ...en, broken: "{count, plural, one {# item}" },
    de: { "cart.title": "{name", "cart.count": 3 },
  });
  assert.deepEqual(
    report.problems.map(({ code, language, key }) => ({ code, language, key })),
    [
      { code: "invalid_message", language: "en", key: "broken" },
      { code: "invalid_message", language: "de", key: "cart.title" },
      { code: "invalid_message", language: "de", key: "cart.count" },
    ],
  );
});

test("the source and the languages that must be complete must exist", () => {
  const report = checkCatalogs({ de: { a: "A" } }, { complete: ["fr"] });
  assert.deepEqual(report.problems, [
    { code: "missing_language", language: "en", message: "has no catalog" },
    { code: "missing_language", language: "fr", message: "has no catalog" },
  ]);
});

test("another source language", () => {
  const report = checkCatalogs({ de: { a: "A", b: "B" }, en: { a: "A" } }, { source: "de" });
  assert.deepEqual(report, { source: "de", problems: [], missing: { en: ["b"] } });
});
