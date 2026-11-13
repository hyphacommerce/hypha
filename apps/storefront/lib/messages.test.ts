import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";

import { createTranslator } from "@hyphacommerce/i18n";

import { PSEUDO, SHARED_NAMESPACES, en, isKey, loadMessages, pick, writtenIn } from "./messages.ts";

/** The keys of a language's catalog files, as the catalog check reads them. */
function keysOnDisk(language: string): string[] {
  const folder = new URL(`../messages/${language}/`, import.meta.url);
  return readdirSync(folder)
    .filter((file) => file.endsWith(".json"))
    .flatMap((file) => {
      const messages: unknown = JSON.parse(readFileSync(new URL(file, folder), "utf8"));
      return Object.keys(Object(messages)).map((key) => `${file.slice(0, -5)}.${key}`);
    })
    .toSorted();
}

test("every catalog file is imported: a namespace added to a folder is one the app reads", async () => {
  assert.deepEqual(Object.keys(en).toSorted(), keysOnDisk("en"));
  assert.deepEqual(Object.keys(await loadMessages("sr-Latn")).toSorted(), keysOnDisk("sr-Latn"));
});

test("English is the catalog for a language without one", async () => {
  assert.equal(await loadMessages("en"), en);
  assert.equal(await loadMessages("de"), en);
});

test("Serbian in Latin script has every English message", async () => {
  const messages = await loadMessages("sr-Latn");
  assert.deepEqual(Object.keys(messages).toSorted(), Object.keys(en).toSorted());
  assert.equal(messages["cart.title"], "Korpa");
});

test("a language with a region reads its language's catalog", async () => {
  assert.equal(await loadMessages("sr-Latn-RS"), await loadMessages("sr-Latn"));
});

test("a page is formatted in the language its catalog is written in", () => {
  assert.equal(writtenIn("sr-Latn"), "sr-Latn");
  assert.equal(writtenIn("sr-Latn-RS"), "sr-Latn-RS");
  // Plain Serbian formats in Cyrillic: beside the Latin catalog, Latin it is.
  assert.equal(writtenIn("sr"), "sr-Latn");
  assert.equal(writtenIn("en-GB"), "en-GB");
  assert.equal(writtenIn("de"), "de");
});

test("Serbian counts one, few and other", async () => {
  const t = createTranslator("sr-Latn", await loadMessages("sr-Latn"));
  const total = (count: number) => t("listing.total", { count });
  assert.deepEqual([1, 2, 5, 21, 22].map(total), [
    "1 proizvod",
    "2 proizvoda",
    "5 proizvoda",
    "21 proizvod",
    "22 proizvoda",
  ]);
  assert.equal(t("header.cart_count", { count: 0 }), "Korpa");
  assert.equal(t("header.cart_count", { count: 3 }), "Korpa, 3 artikla");
  assert.equal(t("header.cart_count", { count: 5 }), "Korpa, 5 artikala");
});

test("the pseudo-language accents English and keeps its placeholders", async () => {
  const t = createTranslator(PSEUDO, await loadMessages(PSEUDO));
  assert.notEqual(t("cart.title"), en["cart.title"]);
  assert.match(t("checkout.pay", { total: "€10.00" }), /€10\.00/);
});

test("the browser is sent only the namespaces asked for, English where a language lacks one", () => {
  const sent = pick({ "cart.title": "Korpa" }, ["cart"]);
  assert.equal(sent["cart.title"], "Korpa");
  assert.equal(sent["cart.total"], en["cart.total"]);
  assert.ok(Object.keys(sent).every((key) => key.startsWith("cart.")));
});

test("no page is sent the errors or the checkout's messages unasked", () => {
  const sent = Object.keys(pick(en, SHARED_NAMESPACES));
  assert.ok(sent.length > 0);
  assert.ok(!sent.some((key) => /^(errors|checkout|account|order)\./.test(key)));
});

test("a key built from an API value is checked against the catalog", () => {
  assert.equal(isKey("order.payment_paid"), true);
  assert.equal(isKey("order.payment_something_new"), false);
  assert.equal(isKey("toString"), false);
});
