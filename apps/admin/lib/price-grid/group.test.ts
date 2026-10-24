import assert from "node:assert/strict";
import { test } from "node:test";

import type { Adjustment } from "../price-lists.ts";
import { type FixedItem, type VariantItem, groupVariants, productIdsOf } from "./group.ts";

const eur = (amount: number) => ({ amount, currency: "EUR" });
const mug = { id: "prod_mug", title: "Mug", handle: "mug" };
const cap = { id: "prod_cap", title: "Cap", handle: "cap" };

function variant(id: string, product: VariantItem["product"], title: string | null): VariantItem {
  return { id, sku: null, title, price: eur(1000), compare_at_price: null, product };
}

const plain = { currency: "EUR", adjustment: null };

test("consecutive variants of a product are one product, in the page's order", () => {
  const items = [
    variant("var_1", mug, "S"),
    variant("var_2", mug, "M"),
    variant("var_3", cap, null),
  ];
  const products = groupVariants(plain, items, new Map(), new Map(), null);
  assert.deepEqual(
    products.map((p) => [p.id, p.handle, p.variants.map((v) => v.variant)]),
    [
      ["prod_mug", "mug", ["var_1", "var_2"]],
      ["prod_cap", "cap", ["var_3"]],
    ],
  );
});

test("a product split across two pages shows on each with the variants there", () => {
  const first = groupVariants(plain, [variant("var_1", mug, "S")], new Map(), new Map(), null);
  const second = groupVariants(plain, [variant("var_2", mug, "M")], new Map(), new Map(), null);
  assert.deepEqual(
    first.map((p) => [p.title, p.variants.map((v) => v.variant)]),
    [["Mug", ["var_1"]]],
  );
  assert.deepEqual(
    second.map((p) => [p.title, p.variants.map((v) => v.variant)]),
    [["Mug", ["var_2"]]],
  );
});

test("a variant's fixed price is the list's for that variant", () => {
  const fixed = new Map<string, FixedItem>([
    ["var_2", { variant: "var_2", price: eur(800), breaks: [] }],
  ]);
  const [product] = groupVariants(
    plain,
    [variant("var_1", mug, "S"), variant("var_2", mug, "M")],
    fixed,
    new Map(),
    null,
  );
  assert.equal(product?.variants[0]?.fixed, null);
  assert.deepEqual(product?.variants[1]?.fixed, { price: eur(800), compare_at: null, breaks: [] });
});

test("a category adjustment prices only products in a covered category", () => {
  const adjustment: Adjustment = {
    percent_bps: -1000,
    applies_to: { type: "categories", categories: ["pcat_a"] },
  };
  const products = groupVariants(
    { currency: "EUR", adjustment },
    [variant("var_1", mug, null), variant("var_2", cap, null)],
    new Map(),
    new Map([["prod_mug", ["pcat_a"]]]),
    new Set(["pcat_a"]),
  );
  assert.deepEqual(products[0]?.variants[0]?.adjusted_price, eur(900));
  assert.equal(products[1]?.variants[0]?.adjusted_price, null);
});

test("product IDs are distinct, in order", () => {
  const items = [
    variant("var_1", mug, "S"),
    variant("var_2", cap, null),
    variant("var_3", mug, "M"),
  ];
  assert.deepEqual(productIdsOf(items), ["prod_mug", "prod_cap"]);
});
