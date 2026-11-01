import assert from "node:assert/strict";
import { test } from "node:test";

import { convertMinor, poNumber } from "./purchasing.ts";

test("a purchase order's number shows with its prefix", () => {
  assert.equal(poNumber(1001), "PO-1001");
});

test("an amount converts at a rate on integers, rounding half away from zero", () => {
  // $10.00 at 0.9213 is €9.213 → 921 cents.
  assert.equal(convertMinor(1000, "0.9213", 2, 2), 921);
  // ¥1,000 at 0.0065 is €6.50.
  assert.equal(convertMinor(1000, "0.0065", 0, 2), 650);
  // 1 KWD (1000 fils) at 3.25 is $3.25.
  assert.equal(convertMinor(1000, "3.25", 3, 2), 325);
  // 1 cent at 0.5 is half a cent → 1; at 0.49999999 just under → 0.
  assert.equal(convertMinor(1, "0.5", 2, 2), 1);
  assert.equal(convertMinor(1, "0.49999999", 2, 2), 0);
  assert.equal(convertMinor(123_456_789, "1", 2, 2), 123_456_789);
});
