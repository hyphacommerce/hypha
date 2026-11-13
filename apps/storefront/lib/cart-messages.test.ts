import assert from "node:assert/strict";
import { test } from "node:test";

import { createTranslator } from "@hyphacommerce/i18n";

import { type AddressFormat, fieldLabel } from "./address.ts";
import {
  type CartMessage,
  type Wordable,
  blockedBy,
  placeMessages,
  placeOf,
  statusOf,
  wordMessage,
} from "./cart-messages.ts";
import { en, loadMessages } from "./messages.ts";

/** A checkout rule's message: shown as the merchant wrote it. */
const message = (path: string, severity = "warning", text = "Check this"): CartMessage => ({
  severity,
  code: "check_this",
  path,
  message: text,
  source: { type: "rule", rule: "crule_1" },
});

const t = createTranslator("en", en);
const sr = createTranslator("sr-Latn", await loadMessages("sr-Latn"), { fallback: en });

/** The US form's labels: a ZIP code and a state. */
const us: AddressFormat = {
  country: "US",
  rows: [["line1"], ["city", "province", "postal_code"]],
  required: ["line1", "city", "province", "postal_code"],
  labels: { line1: "Address", city: "City", province: "State", postal_code: "ZIP code" },
  label_keys: { line1: "address1", city: "city", province: "state", postal_code: "zip_code" },
  postal_code: "required",
  regions: [],
  house_number_required: false,
  autocomplete: false,
};
const usLabel = (field: string) => fieldLabel(t, us, field);
const usLabelSr = (field: string) => fieldLabel(sr, us, field);

/** A built-in problem or message: the API's English, and what it's worded from. */
const builtIn = (code: string, params?: Wordable["params"]): Wordable => ({
  code,
  message: `API: ${code}`,
  params,
});

test("a field's path places the message under that field", () => {
  assert.deepEqual(placeOf("$.shipping_address.line1"), { at: "field", field: "line1" });
});

test("an index path places the message by its group or line", () => {
  assert.deepEqual(placeOf("$.shipping_groups[0]"), { at: "group", index: 0 });
  assert.deepEqual(placeOf("$.lines[12]"), { at: "line", index: 12 });
});

test("an unknown path isn't placed", () => {
  assert.equal(placeOf("$.shipping_address.street"), undefined);
  assert.equal(placeOf("$.lines[01]"), undefined);
  assert.equal(placeOf("$.payment_method"), undefined);
});

test("the cart's messages and unknown ones go to the top", () => {
  const placed = placeMessages([message("$"), message("$.somewhere_new")]);
  assert.equal(placed.top.length, 2);
});

test("messages are grouped by where they're shown", () => {
  const placed = placeMessages([
    message("$.email"),
    message("$.shipping_address"),
    message("$.shipping_address.postal_code"),
    message("$.shipping_groups[1]"),
    message("$.lines[0]"),
  ]);
  assert.equal(placed.email.length, 1);
  assert.equal(placed.address.length, 1);
  assert.equal(placed.fields.postal_code?.length, 1);
  assert.equal(placed.groups[1]?.length, 1);
  assert.equal(placed.lines[0]?.length, 1);
  assert.equal(placed.top.length, 0);
});

test("a field shows its first error before its warnings", () => {
  const status = statusOf(t, [
    message("$.shipping_address.line1", "warning", "Check the street"),
    message("$.shipping_address.line1", "error", "No PO boxes"),
  ]);
  assert.deepEqual(status, { type: "error", message: "No PO boxes" });
});

test("a field's status is worded in the page's language", () => {
  const houseNumber: CartMessage = {
    severity: "warning",
    code: "house_number_missing",
    path: "$.shipping_address.line1",
    message: "Add a house number",
    params: { field: "line1" },
    source: { type: "format" },
  };
  assert.deepEqual(statusOf(sr, [houseNumber]), { type: "warning", message: "Dodajte kućni broj" });
});

test("a severity this build doesn't know shows as a warning", () => {
  assert.deepEqual(statusOf(t, [message("$.email", "notice", "Hi")]), {
    type: "warning",
    message: "Hi",
  });
});

test("required: the field's label, a region chosen and the rest typed", () => {
  assert.equal(wordMessage(t, builtIn("required", { field: "city" }), usLabel), "Enter the city");
  assert.equal(
    wordMessage(t, builtIn("required", { field: "province" }), usLabel),
    "Select the state",
  );
  assert.equal(
    wordMessage(sr, builtIn("required", { field: "postal_code" }), usLabelSr),
    "Popunite polje „ZIP kod“",
  );
  assert.equal(
    wordMessage(sr, builtIn("required", { field: "province" }), usLabelSr),
    "Izaberite: Savezna država",
  );
});

test("too_long: the field and its most, in the language's plurals", () => {
  const long = builtIn("too_long", { field: "line1", max: 255 });
  assert.equal(wordMessage(t, long, usLabel), "The address is too long (at most 255 characters)");
  assert.equal(
    wordMessage(sr, long, usLabelSr),
    "Polje „Adresa“ je predugačko (najviše 255 znakova)",
  );
  assert.equal(
    wordMessage(sr, builtIn("too_long", { field: "line1", max: 2 }), usLabelSr),
    "Polje „Adresa“ je predugačko (najviše 2 znaka)",
  );
});

test("invalid_format: with the country's example where it gives one", () => {
  assert.equal(
    wordMessage(t, builtIn("invalid_format", { field: "postal_code", example: "90210" }), usLabel),
    "Enter a valid ZIP code, like 90210",
  );
  assert.equal(
    wordMessage(t, builtIn("invalid_format", { field: "postal_code" }), usLabel),
    "Enter a valid ZIP code",
  );
  assert.equal(
    wordMessage(
      sr,
      builtIn("invalid_format", { field: "postal_code", example: "90210" }),
      usLabelSr,
    ),
    "Unesite ispravnu vrednost u polje „ZIP kod“, na primer 90210",
  );
});

test("unknown_region: the region's label", () => {
  assert.equal(
    wordMessage(t, builtIn("unknown_region", { field: "province" }), usLabel),
    "Select a valid state",
  );
  assert.equal(
    wordMessage(sr, builtIn("unknown_region", { field: "province" }), usLabelSr),
    "Izaberite ispravnu vrednost u polju „Savezna država“",
  );
});

test("region_mismatch: the region's name, else its code", () => {
  const params = { field: "postal_code", region: "NY", region_name: "New York" };
  assert.equal(
    wordMessage(t, builtIn("region_mismatch", params), usLabel),
    "This ZIP code isn't in New York",
  );
  assert.equal(
    wordMessage(sr, builtIn("region_mismatch", params), usLabelSr),
    "Polje „ZIP kod“ ne odgovara regionu New York",
  );
  assert.equal(
    wordMessage(t, builtIn("region_mismatch", { field: "postal_code", region: "NY" }), usLabel),
    "This ZIP code isn't in NY",
  );
});

test("the codes without a field are worded alike with params or without", () => {
  const words: [string, string, string][] = [
    ["house_number_missing", "Add a house number", "Dodajte kućni broj"],
    [
      "apartment_missing",
      "Add an apartment, suite or unit number.",
      "Dodajte broj stana ili poslovnog prostora.",
    ],
    [
      "address_corrections_suggested",
      "This address may have a mistake. Check the suggested address.",
      "Ova adresa možda sadrži grešku. Proverite predloženu adresu.",
    ],
    [
      "address_ambiguous",
      "We couldn't pin this address down. Check the street, number and postal code.",
      "Nismo uspeli tačno da odredimo ovu adresu. Proverite ulicu, broj i poštanski broj.",
    ],
    [
      "address_undeliverable",
      "We couldn't find this address. Check it before you continue.",
      "Nismo pronašli ovu adresu. Proverite je pre nego što nastavite.",
    ],
    [
      "validation_unavailable",
      "We can't check your order right now. Try again in a moment.",
      "Trenutno ne možemo da proverimo vašu porudžbinu. Pokušajte ponovo za trenutak.",
    ],
  ];
  for (const [code, english, serbian] of words) {
    assert.equal(wordMessage(t, builtIn(code)), english, code);
    assert.equal(wordMessage(sr, builtIn(code)), serbian, code);
    assert.equal(wordMessage(sr, builtIn(code, { field: "line1" }), usLabelSr), serbian, code);
  }
});

test("a field's code without its params, or without a form to label it, keeps its message", () => {
  for (const code of [
    "required",
    "too_long",
    "invalid_format",
    "unknown_region",
    "region_mismatch",
  ]) {
    assert.equal(wordMessage(sr, builtIn(code), usLabelSr), `API: ${code}`, code);
    assert.equal(wordMessage(sr, builtIn(code, { field: "city" })), `API: ${code}`, code);
  }
  assert.equal(wordMessage(sr, builtIn("too_long", { field: "city" }), usLabelSr), "API: too_long");
  assert.equal(
    wordMessage(sr, builtIn("region_mismatch", { field: "postal_code" }), usLabelSr),
    "API: region_mismatch",
  );
});

test("an unknown code keeps its message", () => {
  assert.equal(
    wordMessage(sr, builtIn("something_new", { field: "city" }), usLabelSr),
    "API: something_new",
  );
});

test("a rule's or an app's message is theirs, even under a built-in code", () => {
  const rule = { ...builtIn("house_number_missing"), source: message("$").source };
  assert.equal(wordMessage(sr, rule), "API: house_number_missing");
  const app = { ...builtIn("address_ambiguous"), source: { type: "app" as const, app: "app_x" } };
  assert.equal(wordMessage(sr, app), "API: address_ambiguous");
  const unavailable = { ...builtIn("validation_unavailable"), source: app.source };
  assert.equal(
    wordMessage(sr, unavailable),
    "Trenutno ne možemo da proverimo vašu porudžbinu. Pokušajte ponovo za trenutak.",
  );
});

test("a blocked checkout's messages come from its details", () => {
  const error = {
    code: "checkout_blocked",
    details: { messages: [message("$.shipping_groups[0]", "error"), { path: 1 }] },
  };
  assert.deepEqual(blockedBy(error), [message("$.shipping_groups[0]", "error")]);
});

test("another error blocks nothing", () => {
  assert.deepEqual(blockedBy({ code: "total_changed", details: { messages: [] } }), []);
});
