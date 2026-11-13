import assert from "node:assert/strict";
import { test } from "node:test";

import { sameCatalog } from "./catalog.ts";

test("an equal catalog sent as a new object is the same", () => {
  assert.equal(sameCatalog({ "cart.title": "Cart" }, { "cart.title": "Cart" }), true);
});

test("the same object is the same, missing ones only alike", () => {
  const catalog = { "cart.title": "Cart" };
  assert.equal(sameCatalog(catalog, catalog), true);
  assert.equal(sameCatalog(undefined, undefined), true);
  assert.equal(sameCatalog(catalog, undefined), false);
  assert.equal(sameCatalog(undefined, catalog), false);
});

test("other words, another key or one more key differ", () => {
  const catalog = { "cart.title": "Cart" };
  assert.equal(sameCatalog(catalog, { "cart.title": "Warenkorb" }), false);
  assert.equal(sameCatalog(catalog, { "cart.empty": "Cart" }), false);
  assert.equal(sameCatalog(catalog, { "cart.title": "Cart", "cart.empty": "Empty" }), false);
  assert.equal(sameCatalog({ "cart.title": "Cart", "cart.empty": "Empty" }, catalog), false);
});
