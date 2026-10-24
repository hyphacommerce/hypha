import assert from "node:assert/strict";
import { test } from "node:test";

import { type DiscountDraft, emptyDraft, emptyItems, emptyValue } from "./draft.ts";
import {
  type CheckContext,
  checkDraft,
  clearErrors,
  defaultApiPath,
  errorAt,
  fieldErrors,
  parseOff,
  problemsFromDetails,
} from "./validate.ts";

const ctx: CheckContext = {
  markets: [
    { id: "mkt_ie", name: "Ireland", currency: "EUR" },
    { id: "mkt_jp", name: "Japan", currency: "JPY" },
  ],
  currencies: { EUR: 2, JPY: 0 },
  timeZone: "Europe/Dublin",
};

function valid(change: Partial<DiscountDraft> = {}): DiscountDraft {
  return { ...emptyDraft(), title: "Welcome offer", code: "WELCOME10", ...change };
}

function paths(draft: DiscountDraft): string[] {
  return checkDraft(draft, ctx).map((p) => p.path);
}

function amountOff(amounts: Record<string, string>): DiscountDraft {
  return valid({
    kind: {
      type: "standard",
      product: null,
      order: { value: { ...emptyValue("amount"), amounts } },
      shipping: null,
    },
  });
}

function quantityTier(quantity: string) {
  return { quantity, amounts: {}, value: { ...emptyValue(), percent: "10" } };
}

function subtotalTier(amount: string) {
  return { quantity: "", amounts: { mkt_ie: amount }, value: { ...emptyValue(), percent: "10" } };
}

function mapRewardPath(path: string): string | null {
  return path === "kind.rewards[0].value.percent_bps" ? "kind.product.value.percent" : null;
}

test("the blank draft with a title is valid", () => {
  assert.deepEqual(checkDraft(valid(), ctx), []);
});

test("every problem is reported at once", () => {
  const draft = valid({
    title: "",
    code: "TWO WORDS",
    limits: { totalUses: "0", oncePerCustomer: false },
  });
  assert.deepEqual(paths(draft), ["title", "code", "limits.totalUses"]);
});

test("a codes discount may leave its code for later", () => {
  assert.deepEqual(paths(valid({ code: "" })), []);
});

test("an automatic discount's code isn't checked", () => {
  assert.deepEqual(paths(valid({ method: "automatic", code: "has space" })), []);
});

test("a percentage off is above 0 and at most 100", () => {
  assert.deepEqual(parseOff("0.01"), { ok: true, value: 1 });
  assert.deepEqual(parseOff("100"), { ok: true, value: 10_000 });
  assert.deepEqual(parseOff("0"), { ok: false, error: "More than 0%." });
  assert.deepEqual(parseOff("100.01"), { ok: false, error: "At most 100%." });
  assert.deepEqual(parseOff("-10"), { ok: false, error: "Without a sign: 20 is 20% off." });
});

test("a standard discount needs a reward", () => {
  const draft = valid({ kind: { type: "standard", product: null, order: null, shipping: null } });
  assert.deepEqual(checkDraft(draft, ctx), [
    { path: "kind", message: "Choose what it takes off: products, the order or shipping." },
  ]);
});

test("chosen products must be chosen", () => {
  const draft = valid({
    kind: {
      type: "standard",
      product: { items: emptyItems("products"), value: { ...emptyValue(), percent: "10" } },
      order: null,
      shipping: null,
    },
  });
  assert.deepEqual(checkDraft(draft, ctx), [
    { path: "kind.product.items.products", message: "Choose the products." },
  ]);
});

test("an amount needs one market's amount, and each typed one must parse in its currency", () => {
  assert.deepEqual(paths(amountOff({})), ["kind.order.value.amounts"]);
  assert.deepEqual(paths(amountOff({ mkt_ie: "5" })), []);
  // Yen have no decimals.
  assert.deepEqual(checkDraft(amountOff({ mkt_ie: "5", mkt_jp: "5.5" }), ctx), [
    { path: "kind.order.value.amounts.mkt_jp", message: "This currency has no decimals." },
  ]);
  assert.deepEqual(paths(amountOff({ mkt_ie: "0" })), ["kind.order.value.amounts.mkt_ie"]);
});

test("amounts for markets outside the markets condition aren't checked", () => {
  const draft = valid({
    kind: {
      type: "standard",
      product: null,
      order: { value: { ...emptyValue("amount"), amounts: { mkt_ie: "5", mkt_jp: "nope" } } },
      shipping: null,
    },
  });
  draft.conditions.markets = ["mkt_ie"];
  assert.deepEqual(paths(draft), []);
});

test("a fixed price may be zero; an order reward can't be a fixed price", () => {
  const fixed = valid({
    kind: {
      type: "standard",
      product: {
        items: emptyItems(),
        value: { ...emptyValue("fixed_price"), amounts: { mkt_ie: "0" } },
      },
      order: { value: emptyValue("fixed_price") },
      shipping: null,
    },
  });
  assert.deepEqual(paths(fixed), ["kind.order.value.type"]);
});

test("the cheapest units are a positive whole number", () => {
  const draft = valid({
    kind: {
      type: "standard",
      product: {
        items: emptyItems(),
        value: {
          ...emptyValue("amount"),
          amounts: { mkt_ie: "5" },
          allocation: "cheapest",
          cheapestUnits: "1.5",
        },
      },
      order: null,
      shipping: null,
    },
  });
  assert.deepEqual(paths(draft), ["kind.product.value.cheapestUnits"]);
});

test("a free shipping cap is optional but must parse", () => {
  const draft = valid({
    kind: {
      type: "standard",
      product: null,
      order: null,
      shipping: { value: { ...emptyValue(), percent: "100" }, maxAmount: { mkt_ie: "ten" } },
    },
  });
  assert.deepEqual(paths(draft), ["kind.shipping.maxAmount.mkt_ie"]);
});

test("a minimum subtotal, once switched on, needs an amount", () => {
  const draft = valid();
  draft.conditions.minSubtotal = { enabled: true, amounts: {} };
  draft.conditions.minQuantity = { enabled: true, quantity: "" };
  assert.deepEqual(paths(draft), [
    "conditions.minSubtotal.amounts",
    "conditions.minQuantity.quantity",
  ]);
});

test("a condition lists at most 1,000 IDs", () => {
  const draft = valid();
  draft.conditions.customers = Array.from({ length: 1001 }, (_, i) => `cus_${i}`);
  assert.deepEqual(checkDraft(draft, ctx), [
    { path: "conditions.customers", message: "At most 1,000 customers." },
  ]);
});

test("the end must come after the start", () => {
  const draft = valid({ schedule: { startsAt: "2026-11-01T00:00", endsAt: "2026-11-01T00:00" } });
  assert.deepEqual(checkDraft(draft, ctx), [
    { path: "schedule.endsAt", message: "The end must be after the start." },
  ]);
});

test("tiers go up", () => {
  const draft = valid({
    kind: {
      type: "tiered",
      items: emptyItems(),
      measure: "quantity",
      tiers: [quantityTier("5"), quantityTier("5"), quantityTier("x")],
    },
  });
  assert.deepEqual(paths(draft), ["kind.tiers.1.quantity", "kind.tiers.2.quantity"]);
});

test("subtotal tiers go up in each market", () => {
  const draft = valid({
    kind: {
      type: "tiered",
      items: emptyItems(),
      measure: "subtotal",
      tiers: [subtotalTier("100"), subtotalTier("50")],
    },
  });
  assert.deepEqual(paths(draft), ["kind.tiers.1.amounts.mkt_ie"]);
});

test("buy X get Y needs its quantities", () => {
  const draft = valid({
    kind: {
      type: "buy_x_get_y",
      buys: emptyItems(),
      buy: { type: "quantity", quantity: "0", amounts: {} },
      gets: emptyItems(),
      getQuantity: "",
      value: { ...emptyValue(), percent: "100" },
      usesPerOrder: "",
    },
  });
  assert.deepEqual(paths(draft), ["kind.buy.quantity", "kind.getQuantity"]);
});

// ---- the API's problems ----

test("the API's plain paths land on the draft's fields", () => {
  assert.equal(defaultApiPath("title"), "title");
  assert.equal(defaultApiPath("$.ends_at"), "schedule.endsAt");
  assert.equal(defaultApiPath("/limits/total_uses"), "limits.totalUses");
  assert.equal(defaultApiPath("codes[0]"), "code");
  assert.equal(defaultApiPath("kind.rewards[0].value"), null);
});

test("details as an array of {path, message} become problems", () => {
  const details = [
    { path: "title", message: "must not be blank" },
    { path: "kind.rewards[0].value.amounts", message: "needs an amount." },
    { nonsense: true },
  ];
  assert.deepEqual(problemsFromDetails(details), [
    { path: "title", message: "Must not be blank." },
    { path: "", message: "Needs an amount." },
  ]);
});

test("a page's mapper places the paths it knows", () => {
  assert.deepEqual(
    problemsFromDetails(
      { problems: [{ path: "kind.rewards[0].value.percent_bps", message: "too high" }] },
      mapRewardPath,
    ),
    [{ path: "kind.product.value.percent", message: "Too high." }],
  );
});

test("a single {path, message} is one problem", () => {
  assert.deepEqual(problemsFromDetails({ path: "title", message: "taken" }), [
    { path: "title", message: "Taken." },
  ]);
  assert.deepEqual(problemsFromDetails(null), []);
});

test("field errors keep each path's first problem", () => {
  const errors = fieldErrors([
    { path: "title", message: "First." },
    { path: "title", message: "Second." },
  ]);
  assert.deepEqual(errors, { title: "First." });
});

test("a section shows the first error below it", () => {
  const errors = { "kind.order.value.amounts.mkt_ie": "More than 0." };
  assert.equal(errorAt(errors, "kind.order.value.amounts"), "More than 0.");
  assert.equal(errorAt(errors, "kind.order.value.amount"), undefined);
});

test("changing a field clears its errors and those below it", () => {
  const errors = {
    title: "A",
    "conditions.minSubtotal.amounts.mkt_ie": "B",
    "limits.totalUses": "C",
  };
  assert.deepEqual(clearErrors(errors, ["title", "conditions.minSubtotal"]), {
    "limits.totalUses": "C",
  });
});
