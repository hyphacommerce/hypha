import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type DiscountDraft,
  amountMarkets,
  changedSections,
  classesOf,
  compatible,
  emptyDraft,
  emptyItems,
  emptyValue,
  isDirty,
  phaseOf,
} from "./draft.ts";

const NOW = Date.parse("2026-10-24T12:00:00Z");

function tier(quantity: string) {
  return { quantity, amounts: {}, value: emptyValue() };
}

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

test("an unchanged draft has no changed sections", () => {
  assert.deepEqual(changedSections(emptyDraft(), emptyDraft()), []);
  assert.equal(isDirty(emptyDraft(), emptyDraft()), false);
});

test("changed sections are only those that differ", () => {
  const draft = {
    ...emptyDraft(),
    title: "Welcome",
    limits: { totalUses: "5", oncePerCustomer: false },
  };
  assert.deepEqual(changedSections(emptyDraft(), draft), ["title", "limits"]);
});

test("the order of IDs in a list isn't a change", () => {
  const saved = emptyDraft();
  saved.conditions.markets = ["mkt_a", "mkt_b"];
  const draft = emptyDraft();
  draft.conditions.markets = ["mkt_b", "mkt_a"];
  assert.deepEqual(changedSections(saved, draft), []);
});

test("a blank amount is the same as none", () => {
  const saved = emptyDraft();
  const draft = emptyDraft();
  draft.conditions.minSubtotal.amounts = { mkt_a: " " };
  assert.deepEqual(changedSections(saved, draft), []);
});

test("reordering tiers is a change", () => {
  const saved = withKind({
    type: "tiered",
    items: emptyItems(),
    measure: "quantity",
    tiers: [tier("3"), tier("5")],
  });
  const draft = withKind({
    type: "tiered",
    items: emptyItems(),
    measure: "quantity",
    tiers: [tier("5"), tier("3")],
  });
  assert.deepEqual(changedSections(saved, draft), ["kind"]);
});

test("amounts are typed for the markets the conditions name, or all of them", () => {
  const markets = [{ id: "mkt_a" }, { id: "mkt_b" }];
  assert.deepEqual(amountMarkets({ markets: [] }, markets), markets);
  assert.deepEqual(amountMarkets({ markets: ["mkt_b"] }, markets), [{ id: "mkt_b" }]);
});
