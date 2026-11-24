import assert from "node:assert/strict";
import { test } from "node:test";

import { cartLanguageFor } from "./cart-language.ts";

test("a cart reading in the page's language is left alone", () => {
  assert.equal(cartLanguageFor({ language: "hr" }, "hr", "hr"), undefined);
});

test("a cart reading in another language takes the page's", () => {
  assert.equal(cartLanguageFor({ language: "en" }, "en", "hr"), "hr");
});

test("what the cart reads in is its Content-Language, not the language it keeps", () => {
  // Following the market's default (`null`), it reads in that default.
  assert.equal(cartLanguageFor({ language: null }, "hr", "hr"), undefined);
  assert.equal(cartLanguageFor({ language: null }, "hr", "en"), "en");
});

test("without a Content-Language, the kept language says", () => {
  assert.equal(cartLanguageFor({ language: "en" }, null, "hr"), "hr");
  assert.equal(cartLanguageFor({ language: "hr" }, "", "hr"), undefined);
});

test("a cart whose language nothing says isn't written", () => {
  assert.equal(cartLanguageFor({ language: null }, null, "hr"), undefined);
  assert.equal(cartLanguageFor({}, null, "hr"), undefined);
});

test("spellings of one language are one", () => {
  assert.equal(cartLanguageFor({ language: "sr-Latn" }, "sr-latn", "sr-Latn"), undefined);
  assert.equal(cartLanguageFor({}, "EN-gb", "en-GB"), undefined);
});

test("a region is a language of its own", () => {
  assert.equal(cartLanguageFor({}, "en", "en-GB"), "en-GB");
});
