import assert from "node:assert/strict";
import { test } from "node:test";

import {
  breakLabel,
  lineCompareAt,
  saleLabel,
  shownCompareAt,
  sortedBreaks,
  unitPriceFor,
} from "./prices.ts";

const eur = (amount: number) => ({ amount, currency: "EUR" });
const breaks = [
  { min_quantity: 10, price: eur(800) },
  { min_quantity: 2, price: eur(900) },
];

test("below the first break, one costs the price", () => {
  assert.deepEqual(unitPriceFor(eur(1000), breaks, 1), eur(1000));
});

test("a break applies from its own quantity", () => {
  assert.deepEqual(unitPriceFor(eur(1000), breaks, 2), eur(900));
});

test("the highest break reached wins, in any order", () => {
  assert.deepEqual(unitPriceFor(eur(1000), breaks, 9), eur(900));
  assert.deepEqual(unitPriceFor(eur(1000), breaks, 10), eur(800));
  assert.deepEqual(unitPriceFor(eur(1000), breaks, 50), eur(800));
});

test("without breaks, the price", () => {
  assert.deepEqual(unitPriceFor(eur(1000), [], 10), eur(1000));
});

test("breaks are listed from the smallest quantity", () => {
  assert.deepEqual(
    sortedBreaks(breaks).map((b) => b.min_quantity),
    [2, 10],
  );
});

test("a break reads as a quantity and a unit price", () => {
  assert.equal(breakLabel({ min_quantity: 2, price: eur(900) }, 2), "Buy 2+: €9.00 each");
});

test("a sale reads as was and now", () => {
  assert.equal(saleLabel("€40.00", "€30.00"), "Was €40.00, now €30.00");
});

test("a compare-at is struck through only above the price", () => {
  assert.deepEqual(shownCompareAt(eur(3000), eur(4000)), eur(4000));
  assert.equal(shownCompareAt(eur(3000), eur(3000)), null);
  assert.equal(shownCompareAt(eur(3000), eur(2000)), null);
  assert.equal(shownCompareAt(eur(3000), null), null);
});

test("a compare-at in another currency isn't shown", () => {
  assert.equal(shownCompareAt(eur(3000), { amount: 4000, currency: "USD" }), null);
});

test("a line's compare-at is the unit's times the quantity", () => {
  assert.deepEqual(lineCompareAt(eur(4000), 3, eur(9000)), eur(12000));
  assert.equal(lineCompareAt(eur(4000), 3, eur(12000)), null);
  assert.equal(lineCompareAt(null, 3, eur(9000)), null);
});
