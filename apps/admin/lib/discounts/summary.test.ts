import assert from "node:assert/strict";
import { test } from "node:test";

import { type DiscountDraft, emptyDraft, emptyItems, emptyValue } from "./draft.ts";
import { type SummaryContext, draftSummary, shortMoney } from "./summary.ts";
import { buyXGetY, freeShipping, volumeTiers } from "./templates.ts";

const ctx: SummaryContext = {
  markets: [
    { id: "mkt_ie", name: "Ireland", currency: "EUR" },
    { id: "mkt_uk", name: "UK", currency: "GBP" },
  ],
  currencies: { EUR: 2, GBP: 2 },
  names: { pcat_kits: "Grow kits", pcat_cultures: "Cultures", mkt_ie: "Ireland" },
  today: "2026-10-24",
};

function productDraft(value: DiscountDraft["kind"]): DiscountDraft {
  return { ...emptyDraft(), kind: value };
}

function fiveOff(allocation: "each" | "cheapest", units = "1") {
  return { ...emptyValue("amount"), amounts: { mkt_ie: "5" }, allocation, cheapestUnits: units };
}

const growKits = { ...emptyItems("categories"), categories: ["pcat_kits"] };

test("the note's example reads as written", () => {
  const draft = productDraft({
    type: "standard",
    product: { items: growKits, value: { ...emptyValue(), percent: "20" } },
    order: null,
    shipping: null,
  });
  draft.conditions.minSubtotal = { enabled: true, amounts: { mkt_ie: "50" } };
  draft.combinesWith.shipping = true;
  draft.limits.oncePerCustomer = true;
  assert.equal(
    draftSummary(draft, ctx),
    "20% off Grow kits · Minimum €50 · Combines with shipping discounts · 1 use per customer",
  );
});

test("whole amounts drop their zeros, others keep their digits", () => {
  assert.equal(shortMoney(5000, "EUR", 2), "€50");
  assert.equal(shortMoney(1250, "EUR", 2), "€12.50");
  assert.equal(shortMoney(500, "JPY", 0), "JP¥500");
});

test("amounts in two currencies show both", () => {
  const draft = productDraft({
    type: "standard",
    product: null,
    order: { value: { ...emptyValue("amount"), amounts: { mkt_ie: "15", mkt_uk: "13" } } },
    shipping: null,
  });
  assert.equal(
    draftSummary(draft, ctx),
    "€15 or £13 off the order · Doesn't combine with other discounts",
  );
});

test("amounts off each item and the cheapest name their allocation", () => {
  const each = productDraft({
    type: "standard",
    product: { items: growKits, value: fiveOff("each") },
    order: null,
    shipping: null,
  });
  assert.match(draftSummary(each, ctx), /^€5 off each item in Grow kits · /);
  const cheapest = productDraft({
    type: "standard",
    product: { items: emptyItems(), value: fiveOff("cheapest", "2") },
    order: null,
    shipping: null,
  });
  assert.match(draftSummary(cheapest, ctx), /^€5 off the 2 cheapest items · /);
});

test("a fixed price names its price, and sale items left out say so", () => {
  const draft = productDraft({
    type: "standard",
    product: {
      items: { ...growKits, excludeOnSale: true },
      value: { ...emptyValue("fixed_price"), amounts: { mkt_ie: "5" } },
    },
    order: null,
    shipping: null,
  });
  assert.match(draftSummary(draft, ctx), /^Grow kits for €5 each not on sale · /);
});

test("several rewards join with a plus", () => {
  const draft = productDraft({
    type: "standard",
    product: { items: growKits, value: { ...emptyValue(), percent: "20" } },
    order: null,
    shipping: { value: { ...emptyValue(), percent: "100" }, maxAmount: { mkt_ie: "10" } },
  });
  assert.match(draftSummary(draft, ctx), /^20% off Grow kits \+ Free shipping up to €10 · /);
});

test("free shipping over a minimum, combining with product and order discounts", () => {
  const draft = freeShipping();
  draft.conditions.minSubtotal.amounts = { mkt_ie: "50" };
  assert.equal(
    draftSummary(draft, ctx),
    "Free shipping · Minimum €50 · Combines with product and order discounts",
  );
});

test("an unfinished value says so rather than guessing", () => {
  const draft = productDraft({
    type: "standard",
    product: { items: emptyItems(), value: { ...emptyValue(), percent: "abc" } },
    order: null,
    shipping: null,
  });
  assert.equal(draftSummary(draft, ctx), "Nothing off yet · Doesn't combine with other discounts");
});

test("conditions, limits and the window follow the value", () => {
  const draft = emptyDraft();
  draft.conditions.markets = ["mkt_ie"];
  draft.conditions.signedIn = true;
  draft.limits.totalUses = "100";
  draft.schedule = { startsAt: "2026-11-01T00:00", endsAt: "2027-01-31T23:59" };
  draft.status = "draft";
  assert.equal(
    draftSummary(draft, ctx),
    "10% off every product · In Ireland · Signed-in customers only · Doesn't combine with other discounts · 100 uses in total · 1 Nov – 31 Jan 2027 · Draft",
  );
});

test("an unnamed list counts its IDs", () => {
  const draft = emptyDraft();
  draft.conditions.customerGroups = ["cgrp_1"];
  assert.match(draftSummary(draft, ctx), / · For 1 customer group · /);
});

test("buy X get Y reads as an offer", () => {
  const draft = buyXGetY();
  if (draft.kind.type === "buy_x_get_y") {
    draft.kind.buys = growKits;
    draft.kind.gets = { ...emptyItems("categories"), categories: ["pcat_cultures"] };
  }
  assert.match(
    draftSummary(draft, ctx),
    /^Buy 1 of Grow kits, get 1 of Cultures free \(up to 1 per order\) · /,
  );
});

test("volume tiers list their thresholds", () => {
  const draft = volumeTiers();
  if (draft.kind.type === "tiered") draft.kind.items = growKits;
  assert.match(draftSummary(draft, ctx), /^Buy 3\+ 10% off, 5\+ 15% off on Grow kits · /);
});
