import assert from "node:assert/strict";
import { test } from "node:test";

import { classesOf } from "./draft.ts";
import { TEMPLATES, templateOf } from "./templates.ts";

const defaults = { discount_free_shipping_title: "Free shipping" };

test("every template's key is unique and found by it", () => {
  const keys = TEMPLATES.map((t) => t.key);
  assert.equal(new Set(keys).size, keys.length);
  for (const key of keys) assert.equal(templateOf(key)?.key, key);
  assert.equal(templateOf("nope"), undefined);
});

test("each template fills the kind and classes its name promises", () => {
  const classes = Object.fromEntries(
    TEMPLATES.map((t) => [
      t.key,
      [t.make(defaults).kind.type, ...classesOf(t.make(defaults).kind)],
    ]),
  );
  assert.deepEqual(classes, {
    amount_off_products: ["standard", "product"],
    amount_off_order: ["standard", "order"],
    buy_x_get_y: ["buy_x_get_y", "product"],
    volume: ["tiered", "product"],
    free_shipping: ["standard", "shipping"],
  });
});

test("a template's discount never combines with its own class", () => {
  for (const t of TEMPLATES) {
    const draft = t.make(defaults);
    for (const c of classesOf(draft.kind)) assert.equal(draft.combinesWith[c], false, t.key);
  }
});

test("each call gives a fresh draft", () => {
  const [first] = TEMPLATES;
  assert.ok(first);
  const a = first.make(defaults);
  a.conditions.markets.push("mkt_a");
  assert.deepEqual(first.make(defaults).conditions.markets, []);
});

test("free shipping starts with a minimum to fill in", () => {
  const draft = templateOf("free_shipping")?.make(defaults);
  assert.equal(draft?.conditions.minSubtotal.enabled, true);
});
