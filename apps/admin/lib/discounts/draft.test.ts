import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type DiscountDraft,
  amountMarkets,
  classesOf,
  compatible,
  emptyDraft,
  emptyItems,
  emptyValue,
  phaseOf,
} from "./draft.ts";

const NOW = Date.parse("2026-10-24T12:00:00Z");

function withKind(kind: DiscountDraft["kind"]): DiscountDraft {
  return { ...emptyDraft(), kind };
}

test("a standard draft's classes are its rewards, in their fixed order", () => {
  const draft = withKind({
    type: "standard",
    product: null,
    order: { value: emptyValue() },
    shipping: { value: emptyValue(), maxAmount: {} },
  });
  assert.deepEqual(classesOf(draft.kind), ["order", "shipping"]);
});

test("buy X get Y and tiers are product discounts", () => {
  const tiered = withKind({ type: "tiered", items: emptyItems(), measure: "quantity", tiers: [] });
  assert.deepEqual(classesOf(tiered.kind), ["product"]);
});

test("two discounts combine only when each allows every class of the other", () => {
  const product = { classes: ["product"] as const, combinesWith: ["shipping"] as const };
  const shipping = { classes: ["shipping"] as const, combinesWith: ["product"] as const };
  const order = { classes: ["order"] as const, combinesWith: ["product", "shipping"] as const };
  assert.equal(compatible(product, shipping), true);
  // The order discount allows product, but the product one doesn't allow order.
  assert.equal(compatible(product, order), false);
});

test("a draft is a draft whatever its window", () => {
  assert.equal(phaseOf("draft", null, "2020-01-01T00:00:00Z", NOW), "draft");
});

test("an active discount is scheduled before its start and expired at its end", () => {
  assert.equal(phaseOf("active", "2026-10-25T00:00:00Z", null, NOW), "scheduled");
  assert.equal(phaseOf("active", null, "2026-10-24T12:00:00Z", NOW), "expired");
  assert.equal(phaseOf("active", "2026-10-24T12:00:00Z", null, NOW), "active");
});

test("amounts are typed for the markets the conditions name, or all of them", () => {
  const markets = [{ id: "mkt_a" }, { id: "mkt_b" }];
  assert.deepEqual(amountMarkets({ markets: [] }, markets), markets);
  assert.deepEqual(amountMarkets({ markets: ["mkt_b"] }, markets), [{ id: "mkt_b" }]);
});
