import assert from "node:assert/strict";
import { test } from "node:test";

import { createTranslator } from "@hyphacommerce/i18n";

import { type AddressFormat, fieldLabel, inSentence, problemsOf } from "./address.ts";
import { en, loadMessages } from "./messages.ts";

const t = createTranslator("en", en);
const sr = createTranslator("sr-Latn", await loadMessages("sr-Latn"), { fallback: en });

/** A form whose every field's label has `key`, with `label` as the API's English. */
const formWith = (key: string | null, label = "Somewhere"): AddressFormat => ({
  country: "XX",
  rows: [["city"]],
  required: [],
  labels: { city: label },
  label_keys: { city: key },
  postal_code: "optional",
  regions: [],
  house_number_required: false,
  autocomplete: false,
});

/** The API's English label for each key (`hypha-addresses`' `LabelKey::label`). */
const API_LABELS: Record<string, string> = {
  first_name: "First name",
  last_name: "Last name",
  company: "Company",
  address1: "Address",
  address2: "Apartment, suite, etc.",
  street_and_house_number: "Street and house number",
  full_address: "Full address",
  additional_address: "Additional address",
  city: "City",
  city_town: "City/town",
  city_ward_town_village: "City/ward/town/village",
  ward: "Ward",
  district: "District",
  suburb: "Suburb",
  area: "Area",
  region: "Region",
  province: "Province",
  state: "State",
  state_territory: "State/territory",
  county: "County",
  prefecture: "Prefecture",
  emirate: "Emirate",
  governorate: "Governorate",
  department: "Department",
  postal_code: "Postal code",
  postcode: "Postcode",
  zip_code: "ZIP code",
  pin_code: "PIN code",
  phone: "Phone",
};

test("every label key reads in English as the API words it", () => {
  for (const [key, label] of Object.entries(API_LABELS)) {
    assert.equal(fieldLabel(t, formWith(key), "city"), label, key);
  }
});

test("every label key has its own Serbian", async () => {
  const catalog = await loadMessages("sr-Latn");
  for (const key of Object.keys(API_LABELS)) {
    assert.equal(fieldLabel(sr, formWith(key), "city"), catalog[`address.label_${key}`], key);
  }
  assert.equal(fieldLabel(sr, formWith("zip_code"), "city"), "ZIP kod");
  assert.equal(fieldLabel(sr, formWith("prefecture"), "city"), "Prefektura");
});

test("a label without a key, or with one this build doesn't know, is the API's", () => {
  assert.equal(fieldLabel(sr, formWith(null, "Neighbourhood"), "city"), "Neighbourhood");
  assert.equal(fieldLabel(sr, formWith("barangay", "Barangay"), "city"), "Barangay");
});

test("a format from an API before label keys reads its English labels", () => {
  const older = formWith("city", "Town");
  Reflect.deleteProperty(older, "label_keys");
  assert.equal(fieldLabel(sr, older, "city"), "Town");
});

test("a field the format doesn't label is named as it is, the country by the catalog", () => {
  assert.equal(fieldLabel(sr, formWith("city"), "phone"), "phone");
  assert.equal(fieldLabel(sr, formWith("city"), "country"), "Zemlja");
});

test("a label inside a sentence starts lower case, unless it starts with an acronym", () => {
  assert.equal(inSentence("State", "en"), "state");
  assert.equal(inSentence("ZIP code", "en"), "ZIP code");
  assert.equal(inSentence("Poštanski broj", "sr-Latn"), "poštanski broj");
  assert.equal(inSentence("A", "en"), "A");
  assert.equal(inSentence("", "en"), "");
});

test("an address's problems keep their code and params, by field", () => {
  const problems = problemsOf({
    code: "invalid_address",
    details: {
      problems: [
        {
          field: "postal_code",
          code: "invalid_format",
          message: "Enter a valid ZIP code, like 90210",
          params: { field: "postal_code", example: "90210", extra: 1 },
        },
        { field: "postal_code", code: "required", message: "Enter a ZIP code" },
        { field: "city", code: "required", message: "Enter a city" },
        { field: "street", code: "required", message: "Enter a street" },
        { field: "line1", message: "No code" },
      ],
    },
  });
  assert.deepEqual(problems, {
    postal_code: {
      code: "invalid_format",
      message: "Enter a valid ZIP code, like 90210",
      params: { field: "postal_code", example: "90210" },
    },
    city: { code: "required", message: "Enter a city" },
  });
});

test("another error has no address problems", () => {
  assert.equal(problemsOf({ code: "cart_not_found" }), undefined);
});
