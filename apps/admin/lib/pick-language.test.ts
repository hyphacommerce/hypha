import assert from "node:assert/strict";
import { test } from "node:test";

import { formatLocale, pickLanguage } from "./pick-language.ts";

const available = ["en", "sr-Latn", "de"];

test("the choice on this browser wins over the browser's languages", () => {
  assert.equal(pickLanguage({ cookie: "de", acceptLanguage: "sr-Latn,en;q=0.8" }, available), "de");
});

test("the signed-in user's language wins over this browser's choice and its languages", () => {
  assert.equal(
    pickLanguage({ user: "sr-Latn", cookie: "de", acceptLanguage: "de,en;q=0.8" }, available),
    "sr-Latn",
  );
});

test("the user's language is matched against the catalogs: sr reads sr-Latn", () => {
  assert.equal(pickLanguage({ user: "sr", cookie: "de" }, available), "sr-Latn");
  assert.equal(pickLanguage({ user: "de-AT" }, available), "de");
});

test("a user's language the admin has no catalog for falls to the cookie, then the browser", () => {
  assert.equal(pickLanguage({ user: "fr", cookie: "de", acceptLanguage: "sr" }, available), "de");
  assert.equal(pickLanguage({ user: "fr", acceptLanguage: "sr" }, available), "sr-Latn");
  assert.equal(pickLanguage({ user: "fr" }, available), "en");
});

test("without a user's language (signed out, or none chosen) the cookie decides", () => {
  assert.equal(pickLanguage({ user: null, cookie: "de", acceptLanguage: "sr" }, available), "de");
});

test("the pseudo-language in the cookie wins over the user's language, where it's offered", () => {
  assert.equal(pickLanguage({ user: "de", cookie: "en-XA" }, [...available, "en-XA"]), "en-XA");
  assert.equal(pickLanguage({ user: "de", cookie: "en-XA" }, available), "de");
});

test("a choice is matched in the catalogs' spelling", () => {
  assert.equal(pickLanguage({ cookie: "sr-latn" }, available), "sr-Latn");
});

test("a choice the admin has no catalog for falls to the browser's languages", () => {
  assert.equal(pickLanguage({ cookie: "fr", acceptLanguage: "de-AT,en;q=0.5" }, available), "de");
});

test("a choice that isn't a language tag is ignored", () => {
  assert.equal(pickLanguage({ cookie: "not a tag!", acceptLanguage: "de" }, available), "de");
});

test("the browser's first language the admin has is taken", () => {
  assert.equal(
    pickLanguage({ acceptLanguage: "fr-FR,fr;q=0.9,de;q=0.8,en;q=0.7" }, available),
    "de",
  );
});

test("nothing wanted, or nothing the admin has, is English", () => {
  assert.equal(pickLanguage({}, available), "en");
  assert.equal(pickLanguage({ cookie: null, acceptLanguage: "ja,zh;q=0.9" }, available), "en");
});

test("the pseudo-language is picked only where it's offered", () => {
  assert.equal(pickLanguage({ cookie: "en-XA" }, [...available, "en-XA"]), "en-XA");
  assert.equal(pickLanguage({ cookie: "en-XA" }, available), "en");
});

test("a Serbian browser gets Serbian in Latin script, though it lists English too", () => {
  assert.equal(pickLanguage({ acceptLanguage: "sr-RS,sr;q=0.9" }, available), "sr-Latn");
  assert.equal(
    pickLanguage({ acceptLanguage: "sr-RS,sr;q=0.9,en-US;q=0.8,en;q=0.7" }, available),
    "sr-Latn",
  );
});

test("English and the pseudo-language format the British way, others their own", () => {
  assert.equal(formatLocale("en"), "en-GB");
  assert.equal(formatLocale("en-XA"), "en-GB");
  assert.equal(formatLocale("sr-Latn"), "sr-Latn");
});
