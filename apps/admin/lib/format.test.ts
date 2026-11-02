import assert from "node:assert/strict";
import { test } from "node:test";

import { formatUnitCost, parseUnitCost, unitCostInput, unitCostTotal } from "./format.ts";

const currencies = { EUR: 2, JPY: 0, KWD: 3 };

test("a unit cost reads in the major unit with its extra decimals", () => {
  assert.equal(unitCostInput({ amount: "412.5", currency: "EUR" }, currencies), "4.125");
  assert.equal(unitCostInput({ amount: "0.35", currency: "EUR" }, currencies), "0.0035");
  assert.equal(unitCostInput({ amount: "1000", currency: "EUR" }, currencies), "10");
  assert.equal(unitCostInput({ amount: "1000", currency: "JPY" }, currencies), "1000");
  assert.equal(unitCostInput({ amount: "1.5", currency: "KWD" }, currencies), "0.0015");
  assert.equal(formatUnitCost({ amount: "412.5", currency: "EUR" }, currencies), "€4.125");
  assert.equal(formatUnitCost({ amount: "400", currency: "EUR" }, currencies), "€4.00");
});

test("typed costs become minor units as text", () => {
  assert.equal(parseUnitCost("4.125", "EUR", currencies), "412.5");
  assert.equal(parseUnitCost("0.0035", "EUR", currencies), "0.35");
  assert.equal(parseUnitCost("10", "EUR", currencies), "1000");
  assert.equal(parseUnitCost("10,50", "EUR", currencies), "1050");
  assert.equal(parseUnitCost("0", "EUR", currencies), "0");
  assert.equal(parseUnitCost("1000.5", "JPY", currencies), "1000.5");
  assert.equal(parseUnitCost("0.000001", "EUR", currencies), "0.0001");
  assert.equal(parseUnitCost("0.0000001", "EUR", currencies), null);
  assert.equal(parseUnitCost("-1", "EUR", currencies), null);
  assert.equal(parseUnitCost("abc", "EUR", currencies), null);
});

test("a total rounds half away from zero once", () => {
  assert.deepEqual(unitCostTotal({ amount: "0.35", currency: "EUR" }, 3), {
    amount: 1,
    currency: "EUR",
  });
  assert.deepEqual(unitCostTotal({ amount: "0.35", currency: "EUR" }, 10), {
    amount: 4,
    currency: "EUR",
  });
  assert.deepEqual(unitCostTotal({ amount: "412.5", currency: "EUR" }, 3), {
    amount: 1238,
    currency: "EUR",
  });
});
