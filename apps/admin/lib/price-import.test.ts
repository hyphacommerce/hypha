import assert from "node:assert/strict";
import { test } from "node:test";

import type { ImportRow } from "./price-csv.ts";
import type { PriceGridDiff } from "./price-grid/types.ts";
import { importSteps, stepDiff, stepFailures } from "./price-import.ts";

const row = (line: number, status: ImportRow["status"], variant?: string): ImportRow => ({
  line,
  status,
  variant,
  reasons: [],
});

const eur = (amount: number) => ({ amount, currency: "EUR" });
const upsert = (variant: string) => ({
  variant,
  price: eur(100),
  compare_at: null,
  breaks: [],
});

test("steps hold at most their size of changes, other rows not counted", () => {
  const rows = [
    row(2, "changed", "var_a"),
    row(3, "unchanged", "var_b"),
    row(4, "changed", "var_c"),
    row(5, "invalid"),
    row(6, "changed", "var_d"),
  ];
  assert.deepEqual(importSteps(rows, 2), [
    { from: 2, to: 4, changes: 2 },
    { from: 6, to: 6, changes: 1 },
  ]);
  assert.deepEqual(importSteps(rows, 3), [{ from: 2, to: 6, changes: 3 }]);
});

test("a file with nothing to change takes no step", () => {
  assert.deepEqual(importSteps([row(2, "unchanged", "var_a"), row(3, "unmatched")], 1000), []);
  assert.deepEqual(importSteps([], 1000), []);
});

test("a step's diff is its own lines' changes, both ends included", () => {
  const rows = [
    row(2, "changed", "var_a"),
    row(3, "changed", "var_b"),
    row(4, "changed", "var_c"),
    row(5, "changed", "var_d"),
  ];
  const diff: PriceGridDiff = {
    upserts: [upsert("var_a"), upsert("var_b"), upsert("var_c")],
    clears: [{ variant: "var_d" }],
  };
  assert.deepEqual(stepDiff(rows, diff, { from: 3, to: 5 }), {
    upserts: [upsert("var_b"), upsert("var_c")],
    clears: [{ variant: "var_d" }],
  });
});

test("a step applied before reads as unchanged and sends nothing again", () => {
  // The same file read after the first step: lines 2 and 3 are the list's prices now.
  const rows = [
    row(2, "unchanged", "var_a"),
    row(3, "unchanged", "var_b"),
    row(4, "changed", "var_c"),
  ];
  const diff: PriceGridDiff = { upserts: [upsert("var_c")], clears: [] };
  assert.deepEqual(stepDiff(rows, diff, { from: 2, to: 3 }), { upserts: [], clears: [] });
  assert.deepEqual(stepDiff(rows, diff, { from: 4, to: 4 }), diff);
});

test("refused rows keep their line; a whole failure names every row sent", () => {
  const rows = [row(2, "changed", "var_a"), row(3, "changed", "var_b"), row(4, "invalid", "var_a")];
  const diff: PriceGridDiff = { upserts: [upsert("var_a")], clears: [{ variant: "var_b" }] };
  assert.deepEqual(stepFailures(rows, diff, { rowErrors: { var_a: "Too low." } }), [
    { variant: "var_a", line: 2, error: "Too low." },
  ]);
  assert.deepEqual(stepFailures(rows, diff, { error: "Offline." }), [
    { variant: "var_a", line: 2, error: "Offline." },
    { variant: "var_b", line: 3, error: "Offline." },
  ]);
  assert.deepEqual(stepFailures(rows, diff, {}), []);
});
