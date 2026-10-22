import assert from "node:assert/strict";
import { test } from "node:test";

import { formatMoney } from "./money.ts";

test("amounts are read in the currency's own minor units", () => {
  assert.equal(formatMoney({ amount: 1050, currency: "EUR" }, 2), "€10.50");
  assert.equal(formatMoney({ amount: 1050, currency: "JPY" }, 0), "¥1,050");
});
