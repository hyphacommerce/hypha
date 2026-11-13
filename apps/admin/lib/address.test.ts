import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { createTranslator, mergeCatalogs } from "@hyphacommerce/i18n";

import {
  type AddressT,
  type MessageParams,
  LABEL_KEYS,
  fieldLabel,
  problemsByField,
  problemsOf,
  wordAddressMessage,
} from "./address.ts";

function read(language: string, namespace: string): Record<string, string> {
  const file = join(import.meta.dirname, "..", "messages", language, `${namespace}.json`);
  const parsed: Record<string, string> = JSON.parse(readFileSync(file, "utf8"));
  return parsed;
}
const catalog = (language: string) =>
  mergeCatalogs({ common: read(language, "common"), components: read(language, "components") });
const english = catalog("en");
const translator = (language: string): AddressT =>
  createTranslator(language, catalog(language), { fallback: english });
const en = translator("en");
const sr = translator("sr-Latn");

/** A US-like form: the region is a state, the postal code a ZIP code. */
const us = {
  labels: { line1: "Address", province: "State", postal_code: "ZIP code", city: "City" },
  label_keys: { line1: "address1", province: "state", postal_code: "zip_code", city: "city" },
};

// Labels.

test("each label key's English is the API's label, word for word", () => {
  const source = readFileSync(
    join(import.meta.dirname, "..", "..", "..", "crates", "hypha-addresses", "src", "labels.rs"),
    "utf8",
  );
  const keys = new Map(
    [...source.matchAll(/LabelKey::(\w+) => "([a-z0-9_]+)",/g)].map((m) => [m[1], m[2]]),
  );
  const labels = [...source.matchAll(/LabelKey::(\w+) => "([^"]*[A-Z][^"]*)",/g)];
  assert.equal(labels.length, LABEL_KEYS.length);
  assert.deepEqual([...keys.values()], [...LABEL_KEYS]);
  for (const [, variant, label] of labels) {
    const key = keys.get(variant);
    assert.equal(english[`components.address_label_${key}`], label, `${key}`);
  }
});

test("a field is labelled by its format's key, in the reader's language", () => {
  assert.equal(fieldLabel(en, us, "postal_code"), "ZIP code");
  assert.equal(fieldLabel(sr, us, "postal_code"), "ZIP kôd");
  assert.equal(fieldLabel(sr, us, "province"), "Savezna država");
});

test("a label without a key is shown as the API has it", () => {
  const format = { labels: { city: "Town or village" }, label_keys: { city: null } };
  assert.equal(fieldLabel(sr, format, "city"), "Town or village");
});

test("a key this build doesn't know shows the API's label", () => {
  const format = { labels: { city: "Hamlet" }, label_keys: { city: "hamlet" } };
  assert.equal(fieldLabel(sr, format, "city"), "Hamlet");
});

test("without a format, or a field it doesn't label, the field's default label", () => {
  assert.equal(fieldLabel(sr, undefined, "line1"), "Adresa");
  assert.equal(fieldLabel(en, undefined, "province"), "Region");
  assert.equal(fieldLabel(en, us, "phone"), "Phone");
});

test("the country is named by the shared vocabulary; an unknown field as it is", () => {
  assert.equal(fieldLabel(sr, us, "country"), "Zemlja");
  assert.equal(fieldLabel(en, us, "floor"), "floor");
});

// Problems and messages.

const label = (t: AddressT) => (field: string) => fieldLabel(t, us, field);
const word = (
  t: AddressT,
  code: string,
  params?: MessageParams | null,
  source?: { type: string },
) => wordAddressMessage(t, { code, message: "the API's words", params, source }, label(t));

test("a required field names its label in the country's words", () => {
  assert.equal(word(en, "required", { field: "postal_code" }), "ZIP code is required");
  assert.equal(word(sr, "required", { field: "postal_code" }), "ZIP kôd: obavezno polje");
});

test("too long says the limit, plural by the language", () => {
  assert.equal(
    word(en, "too_long", { field: "city", max: 255 }),
    "City is too long (at most 255 characters)",
  );
  assert.equal(word(sr, "too_long", { field: "city", max: 255 }), "Grad: najviše 255 znakova");
  assert.equal(word(sr, "too_long", { field: "city", max: 22 }), "Grad: najviše 22 znaka");
});

test("an invalid format gives the example where there is one", () => {
  assert.equal(
    word(en, "invalid_format", { field: "postal_code", example: "90210" }),
    "ZIP code should look like 90210",
  );
  assert.equal(word(sr, "invalid_format", { field: "postal_code" }), "ZIP kôd: neispravan format");
});

test("a region mismatch names the region the address gave", () => {
  assert.equal(
    word(sr, "region_mismatch", {
      field: "postal_code",
      region: "NY",
      region_name: "New York",
    }),
    "ZIP kôd: ne pripada regionu New York",
  );
});

test("an unknown region names the field", () => {
  assert.equal(
    word(en, "unknown_region", { field: "province" }),
    "State isn't one of this country's",
  );
});

test("warnings without data are worded by their code alone", () => {
  assert.equal(word(sr, "house_number_missing", { field: "line1" }), "Dodajte kućni broj");
  assert.equal(word(en, "house_number_missing"), "Add a house number");
  assert.equal(
    word(en, "address_undeliverable", null, { type: "verification" }),
    "We couldn't find this address. Check it before you continue.",
  );
  assert.equal(
    word(sr, "apartment_missing", { field: "line2" }, { type: "verification" }),
    "Dodajte broj stana, apartmana ili poslovnog prostora.",
  );
});

test("an unknown code shows the API's words", () => {
  assert.equal(word(sr, "brand_new_code", { field: "city" }), "the API's words");
});

test("a code whose params are missing shows the API's words", () => {
  assert.equal(word(sr, "required"), "the API's words");
  assert.equal(word(sr, "too_long", { field: "city" }), "the API's words");
  assert.equal(word(sr, "region_mismatch", { field: "postal_code" }), "the API's words");
});

test("a checkout rule's or an app's message stays as written, whatever its code", () => {
  assert.equal(word(sr, "required", { field: "city" }, { type: "rule" }), "the API's words");
  assert.equal(word(sr, "house_number_missing", null, { type: "app" }), "the API's words");
  assert.equal(word(sr, "required", { field: "city" }, { type: "format" }), "Grad: obavezno polje");
});

test("problems are kept by field, the first per field, unknown fields left out", () => {
  const problems = problemsByField([
    { field: "city", code: "required", message: "Enter a city", params: { field: "city" } },
    { field: "city", code: "too_long", message: "second", params: { field: "city", max: 255 } },
    { field: "floor", code: "required", message: "Enter a floor", params: { field: "floor" } },
  ]);
  assert.deepEqual(problems, {
    city: { code: "required", message: "Enter a city", params: { field: "city" } },
  });
});

test("a problem's params name its field where the API left it out", () => {
  const problems = problemsByField([
    { field: "postal_code", code: "invalid_format", message: "m", params: { example: "11000" } },
  ]);
  assert.deepEqual(problems.postal_code?.params, { field: "postal_code", example: "11000" });
});

test("a problem without params keeps none, so it's shown in the API's words", () => {
  const problems = problemsByField([{ field: "city", code: "required", message: "Enter a city" }]);
  assert.deepEqual(problems, {
    city: { code: "required", message: "Enter a city", params: undefined },
  });
  const city = problems.city;
  assert.ok(city);
  assert.equal(wordAddressMessage(sr, city, label(sr)), "Enter a city");
});

test("an invalid_address error's problems are read from its details", () => {
  const error = {
    code: "invalid_address",
    details: {
      problems: [
        {
          field: "postal_code",
          code: "invalid_format",
          message: "Enter a valid ZIP code, like 90210",
          params: { field: "postal_code", example: "90210", max: null },
        },
        { field: 3, code: "required", message: "bad" },
      ],
    },
  };
  assert.deepEqual(problemsOf(error), {
    postal_code: {
      code: "invalid_format",
      message: "Enter a valid ZIP code, like 90210",
      params: { field: "postal_code", example: "90210" },
    },
  });
  assert.equal(problemsOf({ code: "not_found", details: error.details }), undefined);
});
