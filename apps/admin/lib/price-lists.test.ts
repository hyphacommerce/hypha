import assert from "node:assert/strict";
import { test } from "node:test";

import type { CategoryNode } from "./categories.ts";
import {
  type ListForm,
  type PriceList,
  adjustAmount,
  adjustedPrice,
  adjustmentLabel,
  audienceLabel,
  candidateReason,
  catalogsInUse,
  checkForm,
  clearErrors,
  coveredCategories,
  coversProduct,
  createRequest,
  emptyForm,
  entryErrors,
  formFromList,
  fromLocalInput,
  parsePercent,
  percentInput,
  toLocalInput,
  updateRequest,
} from "./price-lists.ts";

const eur = (amount: number) => ({ amount, currency: "EUR" });

function node(id: string, children: CategoryNode[] = []): CategoryNode {
  return { id, name: id, handle: id, visible: true, position: 0, children };
}

function list(overrides: Partial<PriceList> = {}): PriceList {
  return {
    id: "pl_1",
    name: "Spring sale",
    description: null,
    currency: "EUR",
    kind: "sale",
    status: "active",
    phase: "active",
    starts_at: null,
    ends_at: null,
    compare_at: "adjusted",
    adjustment: null,
    audience: { groups: [] },
    markets: [],
    fixed_price_count: 0,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

// ---- percentages ----

test("parsePercent reads a discount as negative basis points", () => {
  assert.equal(parsePercent("-20"), -2000);
  assert.equal(parsePercent("5.5"), 550);
  assert.equal(parsePercent("+10"), 1000);
  assert.equal(parsePercent("-12,25"), -1225);
  assert.equal(parsePercent("0"), 0);
});

test("parsePercent refuses more than 100% off, three decimals and words", () => {
  assert.equal(parsePercent("-100"), -10000);
  assert.equal(parsePercent("-100.01"), null);
  assert.equal(parsePercent("1.234"), null);
  assert.equal(parsePercent("ten"), null);
  assert.equal(parsePercent(""), null);
});

test("percentInput is what parsePercent reads back", () => {
  for (const bps of [-2000, 550, 0, -1225]) assert.equal(parsePercent(percentInput(bps)), bps);
});

test("adjustmentLabel says what the adjustment covers", () => {
  assert.equal(adjustmentLabel(null), "No adjustment");
  assert.equal(
    adjustmentLabel({ percent_bps: -2000, applies_to: { type: "all" } }),
    "−20% on every product",
  );
  assert.equal(
    adjustmentLabel({ percent_bps: 500, applies_to: { type: "categories", categories: ["a"] } }),
    "+5% on 1 category",
  );
});

// ---- adjusting ----

test("adjustAmount rounds half away from zero, like Money::adjust", () => {
  assert.equal(adjustAmount(1000, -1000), 900);
  // 999 × 0.85 = 849.15 → 849
  assert.equal(adjustAmount(999, -1500), 849);
  // 5 × 0.9 = 4.5 → 5
  assert.equal(adjustAmount(5, -1000), 5);
  // 15 × 0.9 = 13.5 → 14
  assert.equal(adjustAmount(15, -1000), 14);
  // 1 × 1.49 = 1.49 → 1
  assert.equal(adjustAmount(1, 4900), 1);
  assert.equal(adjustAmount(1, 5000), 2);
});

test("adjustAmount stays exact near the largest amount", () => {
  const max = 2 ** 53 - 1;
  assert.equal(adjustAmount(max, 0), max);
  assert.equal(adjustAmount(max, -5000), Math.round(max / 2));
});

test("coveredCategories takes the categories below the listed ones", () => {
  const tree = [node("a", [node("b", [node("c")])]), node("d")];
  assert.deepEqual(
    coveredCategories({ type: "categories", categories: ["b"] }, tree),
    new Set(["b", "c"]),
  );
  assert.equal(coveredCategories({ type: "all" }, tree), null);
});

test("coversProduct: every product, named products, or products in covered categories", () => {
  const product = { id: "prod_1", categories: ["c"] };
  assert.equal(coversProduct(undefined, product, null), true);
  assert.equal(coversProduct({ type: "products", products: ["prod_1"] }, product, null), true);
  assert.equal(coversProduct({ type: "products", products: ["prod_2"] }, product, null), false);
  const to = { type: "categories" as const, categories: ["b"] };
  assert.equal(coversProduct(to, product, new Set(["b", "c"])), true);
  assert.equal(coversProduct(to, product, new Set(["b"])), false);
});

test("adjustedPrice is null without an adjustment, outside its scope or in another currency", () => {
  const product = { id: "prod_1", categories: [] };
  const adjusted = list({ adjustment: { percent_bps: -2000, applies_to: { type: "all" } } });
  assert.deepEqual(adjustedPrice(adjusted, product, eur(1000), null), eur(800));
  assert.equal(adjustedPrice(list(), product, eur(1000), null), null);
  assert.equal(adjustedPrice(adjusted, product, { amount: 1000, currency: "USD" }, null), null);
  const narrow = list({
    adjustment: { percent_bps: -2000, applies_to: { type: "products", products: ["prod_2"] } },
  });
  assert.equal(adjustedPrice(narrow, product, eur(1000), null), null);
});

// ---- time ----

test("fromLocalInput reads a wall-clock time in the store's zone", () => {
  assert.equal(fromLocalInput("2026-11-27T00:00", "Europe/Berlin"), "2026-11-26T23:00:00Z");
  assert.equal(fromLocalInput("2026-07-01T00:00", "Europe/Berlin"), "2026-06-30T22:00:00Z");
  assert.equal(fromLocalInput("2026-11-27T09:30", "UTC"), "2026-11-27T09:30:00Z");
  assert.equal(fromLocalInput("2026-11-27", "UTC"), null);
});

test("fromLocalInput gets the day daylight saving starts right", () => {
  // Berlin goes from +01:00 to +02:00 at 02:00 on 29 March 2026.
  assert.equal(fromLocalInput("2026-03-29T03:30", "Europe/Berlin"), "2026-03-29T01:30:00Z");
  assert.equal(fromLocalInput("2026-03-29T01:30", "Europe/Berlin"), "2026-03-29T00:30:00Z");
});

test("toLocalInput is the inverse of fromLocalInput", () => {
  assert.equal(toLocalInput("2026-11-26T23:00:00Z", "Europe/Berlin"), "2026-11-27T00:00");
  assert.equal(toLocalInput("2026-06-30T22:00:00Z", "Europe/Berlin"), "2026-07-01T00:00");
  assert.equal(toLocalInput(null, "UTC"), "");
  for (const local of ["2026-01-05T08:15", "2026-08-31T23:59"]) {
    const at = fromLocalInput(local, "America/New_York");
    assert.ok(at);
    assert.equal(toLocalInput(at, "America/New_York"), local);
  }
});

// ---- the form ----

function form(overrides: Partial<ListForm> = {}): ListForm {
  return { ...emptyForm("EUR"), name: "Spring sale", ...overrides };
}

test("checkForm wants a name, a valid percentage and something to apply it to", () => {
  assert.deepEqual(checkForm(form(), "UTC"), {});
  assert.ok(checkForm(form({ name: " " }), "UTC").name);
  assert.ok(checkForm(form({ hasAdjustment: true, percent: "x" }), "UTC").percent);
  assert.ok(
    checkForm(form({ hasAdjustment: true, percent: "-10", scope: "products" }), "UTC").products,
  );
  assert.ok(
    checkForm(form({ hasAdjustment: true, percent: "-10", scope: "categories" }), "UTC").categories,
  );
});

test("checkForm wants the end after the start", () => {
  const errors = checkForm(
    form({ startsAt: "2026-11-27T00:00", endsAt: "2026-11-27T00:00" }),
    "UTC",
  );
  assert.equal(errors.endsAt, "The end must be after the start.");
  assert.deepEqual(
    checkForm(form({ startsAt: "2026-11-27T00:00", endsAt: "2026-11-27T00:01" }), "UTC"),
    {},
  );
});

test("createRequest sends the window in UTC and the adjustment's scope", () => {
  const result = createRequest(
    form({
      kind: "sale",
      description: "  Black Friday ",
      startsAt: "2026-11-27T00:00",
      endsAt: "2026-12-01T00:00",
      hasAdjustment: true,
      percent: "-20",
      scope: "categories",
      categories: ["pcat_1"],
    }),
    "Europe/Berlin",
  );
  assert.deepEqual(result, {
    body: {
      name: "Spring sale",
      currency: "EUR",
      kind: "sale",
      status: "active",
      description: "Black Friday",
      starts_at: "2026-11-26T23:00:00Z",
      ends_at: "2026-11-30T23:00:00Z",
      adjustment: {
        percent_bps: -2000,
        applies_to: { type: "categories", categories: ["pcat_1"] },
      },
    },
  });
});

test("createRequest sends the compare-at mode only for regular prices", () => {
  const sale = createRequest(form({ kind: "sale", compareAt: "hidden" }), "UTC");
  assert.ok("body" in sale && sale.body.compare_at === undefined);
  const override = createRequest(form({ kind: "override", compareAt: "hidden" }), "UTC");
  assert.ok("body" in override && override.body.compare_at === "hidden");
});

test("updateRequest sends nothing for an unchanged form", () => {
  const saved = list({
    starts_at: "2026-11-26T23:00:00Z",
    adjustment: { percent_bps: -2000, applies_to: { type: "products", products: ["b", "a"] } },
  });
  assert.deepEqual(updateRequest(formFromList(saved, "Europe/Berlin"), saved, "Europe/Berlin"), {
    body: {},
  });
});

test("updateRequest clears with null and sends only what changed", () => {
  const saved = list({
    description: "Old",
    ends_at: "2026-12-01T00:00:00Z",
    adjustment: { percent_bps: -2000, applies_to: { type: "all" } },
  });
  const edited = {
    ...formFromList(saved, "UTC"),
    description: "",
    endsAt: "",
    hasAdjustment: false,
  };
  assert.deepEqual(updateRequest(edited, saved, "UTC"), {
    body: { description: null, ends_at: null, adjustment: null },
  });
});

test("updateRequest sends the whole adjustment when its scope changes", () => {
  const saved = list({ adjustment: { percent_bps: -2000, applies_to: { type: "all" } } });
  const edited = { ...formFromList(saved, "UTC"), scope: "products" as const, products: ["p"] };
  assert.deepEqual(updateRequest(edited, saved, "UTC"), {
    body: { adjustment: { percent_bps: -2000, applies_to: { type: "products", products: ["p"] } } },
  });
});

test("createRequest names the audience only when it isn't everyone", () => {
  const everyone = createRequest(form(), "UTC");
  assert.ok("body" in everyone && everyone.body.audience === undefined);
  const group = createRequest(form({ groups: ["cgrp_1"] }), "UTC");
  assert.ok("body" in group && group.body.audience?.groups[0] === "cgrp_1");
});

test("updateRequest sends the audience only when its groups change", () => {
  const saved = list({ audience: { groups: ["cgrp_a", "cgrp_b"] } });
  const reordered = { ...formFromList(saved, "UTC"), groups: ["cgrp_b", "cgrp_a"] };
  assert.deepEqual(updateRequest(reordered, saved, "UTC"), { body: {} });
  const everyone = { ...formFromList(saved, "UTC"), groups: [] };
  assert.deepEqual(updateRequest(everyone, saved, "UTC"), {
    body: { audience: { groups: [] } },
  });
});

test("audienceLabel names up to two groups, else counts them", () => {
  const names = { cgrp_a: "Wholesale", cgrp_b: "VIP" };
  assert.equal(audienceLabel({ groups: [] }, names), "Everyone");
  assert.equal(audienceLabel(undefined, names), "Everyone");
  assert.equal(audienceLabel({ groups: ["cgrp_a", "cgrp_b"] }, names), "Wholesale and VIP");
  assert.equal(audienceLabel({ groups: ["cgrp_x"] }, names), "cgrp_x");
  assert.equal(audienceLabel({ groups: ["a", "b", "c"] }, names), "3 customer groups");
});

test("candidateReason says a list for other groups doesn't price for this buyer", () => {
  assert.equal(
    candidateReason("not_in_audience", "active"),
    "Not for this buyer: it prices only for other customer groups.",
  );
});

// ---- errors ----

test("entryErrors names each refused variant, by variant or by index", () => {
  const sent = [{ variant: "var_a" }, { variant: "var_b" }];
  assert.deepEqual(
    entryErrors(
      {
        entries: [
          { index: 0, variant: "var_a", code: "compare_at_not_above_price", message: "x" },
          { index: 1, code: "price_breaks_not_descending", message: "y" },
        ],
      },
      sent,
    ),
    {
      var_a: "The compare-at price must be above the price.",
      var_b: "Each break's price must be below the one before it, the first below the price.",
    },
  );
  assert.equal(entryErrors({ other: 1 }, sent), null);
  assert.equal(entryErrors(null, sent), null);
});

test("catalogsInUse reads the catalogs holding a list", () => {
  assert.deepEqual(catalogsInUse({ catalogs: ["cat_1", 2, "cat_2"] }), ["cat_1", "cat_2"]);
  assert.deepEqual(catalogsInUse(undefined), []);
});

test("candidateReason explains a list that isn't live by its phase", () => {
  assert.equal(
    candidateReason("not_live", "scheduled"),
    "Not live yet: its window hasn't started.",
  );
  assert.equal(candidateReason("not_live", "draft"), "Not live: it's a draft.");
  assert.equal(candidateReason("won", "active"), "Wins: the lowest price.");
});

test("clearErrors drops the errors of the fields being changed", () => {
  assert.deepEqual(clearErrors({ name: "x", percent: "y" }, { name: "New" }), { percent: "y" });
});
