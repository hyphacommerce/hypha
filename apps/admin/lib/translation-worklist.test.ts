import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import type { TFor } from "./messages";
import {
  type WorklistItem,
  idsToName,
  isStoreWide,
  kindLabel,
  nameKey,
  progressParts,
  shownShare,
  worklistFilters,
  worklistHref,
  worklistQuery,
} from "./translation-worklist.ts";

function read(language: string, namespace: string): Record<string, string> {
  const file = join(import.meta.dirname, "..", "messages", language, `${namespace}.json`);
  const parsed: Record<string, string> = JSON.parse(readFileSync(file, "utf8"));
  return parsed;
}
const catalog = (language: string) =>
  mergeCatalogs({ translations: read(language, "translations") });
const translator = (language: string): TFor<"translations"> =>
  createTranslator(language, catalog(language), { fallback: catalog("en") });

const progress = (total: number, translated: number, outdated: number, inherited = 0) => ({
  language: inherited === 0 ? "de" : "de-AT",
  total,
  translated,
  outdated,
  missing: total - translated,
  inherited,
});

const item = (kind: string, id: string): WorklistItem => ({
  resource: { kind, id },
  language: "de",
  state: "missing",
  total: 3,
  missing: 1,
  outdated: 0,
  inherited: 0,
  fields: [],
});

// ---------- progress ----------

test("progress splits what's translated into current and outdated", () => {
  const parts = progressParts(progress(10, 7, 2));
  assert.deepEqual(
    { current: parts.current, outdated: parts.outdated, missing: parts.missing },
    { current: 5, outdated: 2, missing: 3 },
  );
});

test("the share translated counts outdated translations in", () => {
  assert.equal(progressParts(progress(8, 6, 6)).translated, 0.75);
});

test("the bar's segments add up to its whole width", () => {
  const { shares } = progressParts(progress(3, 2, 1));
  assert.equal(shares.current + shares.outdated + shares.inherited + shares.untranslated, 1);
  assert.equal(shares.untranslated, 1 / 3);
});

test("nothing to translate has no share and an empty bar", () => {
  const parts = progressParts(progress(0, 0, 0));
  assert.equal(parts.translated, null);
  assert.deepEqual(parts.shares, { current: 0, outdated: 0, inherited: 0, untranslated: 0 });
});

test("every field translated is the whole share, with nothing missing", () => {
  const parts = progressParts(progress(4, 4, 0));
  assert.equal(parts.translated, 1);
  assert.equal(parts.missing, 0);
});

test("counts past their bounds are clamped, never drawn past the track", () => {
  const parts = progressParts({
    language: "de",
    total: 4,
    translated: 6,
    outdated: 9,
    missing: 0,
    inherited: 0,
  });
  assert.deepEqual(
    { current: parts.current, outdated: parts.outdated, missing: parts.missing },
    { current: 0, outdated: 4, missing: 0 },
  );
  assert.equal(parts.translated, 1);
});

test("a regional language's inherited fields are among the missing, the rest untranslated", () => {
  const parts = progressParts(progress(300, 12, 2, 270));
  assert.deepEqual(
    {
      current: parts.current,
      outdated: parts.outdated,
      missing: parts.missing,
      inherited: parts.inherited,
      untranslated: parts.untranslated,
    },
    { current: 10, outdated: 2, missing: 288, inherited: 270, untranslated: 18 },
  );
});

test("inherited is drawn as its own segment, and the four fill the bar", () => {
  const { shares } = progressParts(progress(10, 2, 1, 5));
  assert.deepEqual(shares, { current: 0.1, outdated: 0.1, inherited: 0.5, untranslated: 0.3 });
});

test("every missing field inherited leaves nothing in the default language", () => {
  const parts = progressParts(progress(8, 2, 0, 6));
  assert.equal(parts.untranslated, 0);
  assert.equal(parts.shares.untranslated, 0);
});

test("inherited isn't counted into the share translated", () => {
  const parts = progressParts(progress(100, 12, 0, 88));
  assert.equal(parts.translated, 0.12);
  assert.equal(shownShare(parts), 0.12);
});

test("inherited past the missing is clamped to them", () => {
  const parts = progressParts({ ...progress(10, 6, 0), inherited: 9 });
  assert.equal(parts.inherited, 4);
  assert.equal(parts.untranslated, 0);
});

test("the shown share rounds down, so one field missing never reads 100%", () => {
  assert.equal(shownShare(progressParts(progress(1000, 999, 0))), 0.99);
  assert.equal(shownShare(progressParts(progress(4, 4, 1))), 1);
  assert.equal(shownShare(progressParts(progress(1000, 4, 0))), 0);
  assert.equal(shownShare(progressParts(progress(0, 0, 0))), 0);
});

test("the shown share is exact where the fraction isn't (29 of 100 is 29%)", () => {
  assert.equal(shownShare(progressParts(progress(100, 29, 0))), 0.29);
  assert.equal(shownShare(progressParts(progress(100, 57, 3))), 0.57);
});

// ---------- filters → query ----------

test("the asked language is matched whatever its case, in the store's own", () => {
  assert.equal(worklistFilters({ language: "SR-latn" }, ["de", "sr-Latn"]).language, "sr-Latn");
});

test("no language, or one the store lacks, is the store's first", () => {
  assert.equal(worklistFilters({}, ["fr", "de"]).language, "fr");
  assert.equal(worklistFilters({ language: "it" }, ["fr", "de"]).language, "fr");
});

test("a store without other languages has no language to list", () => {
  assert.equal(worklistFilters({ language: "de" }, []).language, undefined);
});

test("the state defaults to missing, and an unknown one too", () => {
  assert.equal(worklistFilters({}, ["de"]).state, "missing");
  assert.equal(worklistFilters({ state: "done" }, ["de"]).state, "missing");
  assert.equal(worklistFilters({ state: "outdated" }, ["de"]).state, "outdated");
});

test("an unknown kind is every kind", () => {
  assert.equal(worklistFilters({ kind: "banana" }, ["de"]).kind, undefined);
  assert.equal(worklistFilters({ kind: "discount" }, ["de"]).kind, "discount");
});

test("the query names the language and state, and the kind and page when given", () => {
  assert.deepEqual(
    worklistQuery({ language: "de", state: "missing", kind: undefined }, { limit: 25 }),
    { language: "de", state: "missing", limit: 25 },
  );
  assert.deepEqual(
    worklistQuery({ language: "de", state: "complete", kind: "product" }, { cursor: "c1" }),
    { language: "de", state: "complete", kind: "product", cursor: "c1" },
  );
});

test("a worklist link carries what it's filtered by", () => {
  assert.equal(worklistHref({ language: "de" }), "/translations?language=de");
  assert.equal(
    worklistHref({ language: "sr-Latn", state: "outdated", kind: "category" }),
    "/translations?language=sr-Latn&state=outdated&kind=category",
  );
});

// ---------- kinds and names ----------

test("a kind is named in the reader's language", () => {
  assert.equal(kindLabel(translator("en"), "shipping_option"), "Shipping option");
  assert.equal(kindLabel(translator("sr-Latn"), "shipping_option"), "Opcija dostave");
});

test("a kind this build doesn't know is shown as the API wrote it", () => {
  assert.equal(kindLabel(translator("en"), "menu"), "menu");
});

test("the store's details and its search filters are named by their kind alone", () => {
  assert.equal(isStoreWide("store"), true);
  assert.equal(isStoreWide("search_facet"), true);
  assert.equal(isStoreWide("product"), false);
});

test("a page's IDs to name are grouped by kind, each once, named kinds only", () => {
  assert.deepEqual(
    idsToName([
      item("product", "prod_1"),
      item("category", "pcat_1"),
      item("product", "prod_2"),
      item("product", "prod_1"),
      item("discount", "disc_1"),
      item("store", "store_1"),
    ]),
    { product: ["prod_1", "prod_2"], category: ["pcat_1"] },
  );
});

test("names are kept by kind and ID together", () => {
  assert.notEqual(nameKey("store", "x"), nameKey("search_facet", "x"));
});
