import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type Drafts,
  type Target,
  applyChanges,
  cellKey,
  checkDraft,
  clearCells,
  fillDown,
  normalizeCell,
  percentOfBase,
  review,
  setTo,
} from "./edits.ts";
import { isSingleRow, variantsById, visibleRows } from "./rows.ts";
import type { PriceGridProduct, PriceGridVariant } from "./types.ts";

const eur = (amount: number) => ({ amount, currency: "EUR" });

function variant(id: string, base: number, fixed?: number): PriceGridVariant {
  return {
    variant: id,
    title: `Size ${id}`,
    sku: `SKU-${id}`,
    base_price: eur(base),
    compare_at_price: null,
    adjusted_price: null,
    fixed: fixed === undefined ? null : { price: eur(fixed), compare_at: null, breaks: [] },
  };
}

const products: PriceGridProduct[] = [
  {
    id: "prod_1",
    title: "Grow kit",
    handle: "grow-kit",
    variants: [variant("var_a", 2000, 1800), variant("var_b", 2500)],
  },
];
const variants = variantsById(products);
const none: Drafts = new Map();

test("a typed price is an upsert; the same price typed differently is nothing", () => {
  const drafts = applyChanges(
    none,
    variants,
    [{ variant: "var_b", column: "fixed_price", text: "22,5" }],
    2,
  );
  assert.deepEqual(review(variants, drafts, "EUR", 2).diff, {
    upserts: [{ variant: "var_b", price: eur(2250), compare_at: null, breaks: [] }],
    clears: [],
  });
  const same = applyChanges(
    none,
    variants,
    [{ variant: "var_a", column: "fixed_price", text: "18" }],
    2,
  );
  assert.equal(same.size, 0);
});

test("emptying a stored fixed price clears it", () => {
  const drafts = applyChanges(
    none,
    variants,
    clearCells([{ variant: "var_a", column: "fixed_price" }]),
    2,
  );
  const { diff, changedVariants, dirty } = review(variants, drafts, "EUR", 2);
  assert.deepEqual(diff, { upserts: [], clears: [{ variant: "var_a" }] });
  assert.equal(changedVariants, 1);
  assert.equal(dirty.has(cellKey("var_a", "fixed_price")), true);
});

test("typing then emptying a cell that had nothing leaves nothing to save", () => {
  let drafts = applyChanges(
    none,
    variants,
    [{ variant: "var_b", column: "fixed_price", text: "9" }],
    2,
  );
  drafts = applyChanges(
    drafts,
    variants,
    [{ variant: "var_b", column: "fixed_price", text: "" }],
    2,
  );
  assert.equal(drafts.size, 0);
});

test("errors stay on their cells and keep the variant out of the diff", () => {
  const drafts = applyChanges(
    none,
    variants,
    [
      { variant: "var_a", column: "fixed_price", text: "12.3456" },
      { variant: "var_b", column: "fixed_compare_at", text: "30" },
    ],
    2,
  );
  const { diff, errors, changedVariants } = review(variants, drafts, "EUR", 2);
  assert.deepEqual(diff, { upserts: [], clears: [] });
  assert.equal(changedVariants, 2);
  assert.equal(errors.get(cellKey("var_a", "fixed_price")), "At most 2 decimal places.");
  assert.equal(
    errors.get(cellKey("var_b", "fixed_price")),
    "Needs a fixed price, or clear the row.",
  );
});

test("a compare-at price must be above the fixed price", () => {
  const draft = { fixed_price: "10", fixed_compare_at: "10", breaks: "" };
  assert.deepEqual(checkDraft(draft, "EUR", 2), {
    kind: "invalid",
    errors: { fixed_compare_at: "Must be above the fixed price." },
  });
  assert.equal(checkDraft({ ...draft, fixed_compare_at: "10.01" }, "EUR", 2).kind, "price");
});

test("breaks are checked against the fixed price in the same draft", () => {
  assert.deepEqual(
    checkDraft({ fixed_price: "10", fixed_compare_at: "", breaks: "5=10" }, "EUR", 2),
    {
      kind: "invalid",
      errors: { breaks: "The price from 5 must be below the fixed price." },
    },
  );
  assert.deepEqual(
    checkDraft({ fixed_price: "10", fixed_compare_at: "", breaks: "5=9; 10=8" }, "EUR", 2),
    {
      kind: "price",
      fixed: {
        price: eur(1000),
        compare_at: null,
        breaks: [
          { min_quantity: 5, price: eur(900) },
          { min_quantity: 10, price: eur(800) },
        ],
      },
    },
  );
});

test("cells normalize so equal prices compare equal", () => {
  assert.equal(normalizeCell("fixed_price", " 12,5 ", 2), "12.50");
  assert.equal(normalizeCell("breaks", "10:8", 2), "10=8.00");
  assert.equal(normalizeCell("fixed_price", "abc", 2), "abc");
});

// A grid of two columns (fixed price, compare-at) over rows: 0 product, 1 var_a, 2 var_b.
const rows = visibleRows(products, new Set());
const columns = ["fixed_price", "fixed_compare_at"] as const;
const targetAt = ({ row, col }: { row: number; col: number }): Target | null => {
  const r = rows[row];
  const column = columns[col];
  return r?.kind === "variant" && column ? { variant: r.variant.variant, column } : null;
};

test("rows: a product with options heads its variants; collapsed, it's alone", () => {
  assert.deepEqual(
    rows.map((r) => r.kind),
    ["product", "variant", "variant"],
  );
  assert.deepEqual(
    visibleRows(products, new Set(["prod_1"])).map((r) => r.kind),
    ["product"],
  );
});

test("rows: a product without options is one row", () => {
  const single: PriceGridProduct = {
    id: "prod_2",
    title: "Scalpel",
    handle: "scalpel",
    variants: [{ ...variant("var_c", 500), title: null }],
  };
  assert.equal(isSingleRow(single), true);
  assert.deepEqual(visibleRows([single], new Set()), [
    { kind: "variant", product: single, variant: single.variants[0], isSingle: true },
  ]);
});

test("fill down copies the first editable cell, skipping the product row", () => {
  const changes = fillDown({ top: 0, left: 0, bottom: 2, right: 0 }, targetAt, () => "18.00");
  assert.deepEqual(changes, [{ variant: "var_b", column: "fixed_price", text: "18.00" }]);
});

test("fill down on one row takes the nearest editable cell above", () => {
  const changes = fillDown({ top: 2, left: 0, bottom: 2, right: 1 }, targetAt, (t) =>
    t.column === "fixed_price" ? "18.00" : "",
  );
  assert.deepEqual(changes, [
    { variant: "var_b", column: "fixed_price", text: "18.00" },
    { variant: "var_b", column: "fixed_compare_at", text: "" },
  ]);
});

test("fill down with nothing above changes nothing", () => {
  assert.deepEqual(
    fillDown({ top: 1, left: 0, bottom: 1, right: 0 }, targetAt, () => "1"),
    [],
  );
});

const targets: Target[] = [
  { variant: "var_a", column: "fixed_price" },
  { variant: "var_b", column: "fixed_price" },
  { variant: "var_b", column: "breaks" },
];

test("set to an amount fills the money cells, not breaks", () => {
  assert.deepEqual(setTo(targets, 1500, 2), [
    { variant: "var_a", column: "fixed_price", text: "15.00" },
    { variant: "var_b", column: "fixed_price", text: "15.00" },
  ]);
});

test("percent of base adjusts each variant's own base price, rounding like the API", () => {
  const odd = variantsById([
    { ...products[0], variants: [variant("var_a", 1005), variant("var_b", 2500)] },
  ]);
  assert.deepEqual(percentOfBase(targets, -1000, odd, "EUR", 2), {
    changes: [
      { variant: "var_a", column: "fixed_price", text: "9.05" },
      { variant: "var_b", column: "fixed_price", text: "22.50" },
    ],
    skipped: 0,
  });
});

test("percent of base skips a base price in another currency", () => {
  const chf = variantsById([
    {
      ...products[0],
      variants: [{ ...variant("var_a", 1000), base_price: { amount: 1000, currency: "CHF" } }],
    },
  ]);
  assert.deepEqual(percentOfBase(targets.slice(0, 1), 500, chf, "EUR", 2), {
    changes: [],
    skipped: 1,
  });
});

test("clear empties every selected cell", () => {
  assert.deepEqual(
    clearCells(targets).map((c) => c.text),
    ["", "", ""],
  );
});
