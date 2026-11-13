import assert from "node:assert/strict";
import { test } from "node:test";

import { createTranslator } from "@hyphacommerce/i18n";

import { PSEUDO, SHARED_NAMESPACES, en, isKey, loadMessages, pick } from "./messages.ts";

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
