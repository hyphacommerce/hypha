import assert from "node:assert/strict";
import { test } from "node:test";

import { TAGS, languageChange } from "./cache-tags.ts";

const translated = (kind: string, id = "x_1") => ({
  resource: { kind, id },
  language: "de",
  fields: ["title"],
});

test("a product's translation expires that product's pages", () => {
  assert.deepEqual(languageChange("translation.updated", translated("product", "prod_1")), {
    tags: [],
    products: ["prod_1"],
  });
  assert.deepEqual(languageChange("translation.deleted", translated("product", "prod_1")), {
    tags: [],
    products: ["prod_1"],
  });
});

test("a category's translation expires the tree and every category page", () => {
  assert.deepEqual(languageChange("translation.updated", translated("category")), {
    tags: [TAGS.categories],
    products: [],
  });
});

test("a definition's, the store's and anything else's translations", () => {
  assert.deepEqual(
    languageChange("translation.updated", translated("custom_field_definition"))?.tags,
    [TAGS.fields],
  );
  assert.deepEqual(languageChange("translation.updated", translated("store"))?.tags, [TAGS.store]);
  assert.deepEqual(languageChange("translation.updated", translated("search_facet"))?.tags, [
    TAGS.listings,
  ]);
  assert.deepEqual(languageChange("translation.deleted", translated("shipping_option"))?.tags, [
    TAGS.listings,
  ]);
});

test("a product translation without an ID expires every product page", () => {
  assert.deepEqual(languageChange("translation.updated", { resource: { kind: "product" } }), {
    tags: [TAGS.products],
    products: [],
  });
});

test("the store's languages changing expires the store and its countries", () => {
  assert.deepEqual(languageChange("store.updated", { fields: ["languages"] }), {
    tags: [TAGS.store],
    products: [],
  });
});

test("other events are left to the webhook's own cases", () => {
  assert.equal(languageChange("product.updated", {}), undefined);
});

test("a discount's or an image's translation expires the pages that show it", () => {
  assert.deepEqual(languageChange("translation.updated", translated("discount"))?.tags, [
    TAGS.products,
    TAGS.listings,
  ]);
  assert.deepEqual(languageChange("translation.updated", translated("asset"))?.tags, [
    TAGS.products,
    TAGS.listings,
    TAGS.categories,
  ]);
});
