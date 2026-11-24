import assert from "node:assert/strict";
import { test } from "node:test";

import { indexable } from "./indexing.ts";

test("a store is indexable unless STOREFRONT_NOINDEX says otherwise", () => {
  assert.equal(indexable(undefined), true);
  assert.equal(indexable(""), true);
  assert.equal(indexable("0"), true);
  assert.equal(indexable("false"), true);
  assert.equal(indexable(" FALSE "), true);
});

test("STOREFRONT_NOINDEX=1 or true keeps the store out of search", () => {
  assert.equal(indexable("1"), false);
  assert.equal(indexable("true"), false);
  assert.equal(indexable("yes"), false);
});
