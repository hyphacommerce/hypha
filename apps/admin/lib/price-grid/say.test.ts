import assert from "node:assert/strict";
import { test } from "node:test";

import { breaksLabel, parseBreaks } from "./breaks.ts";
import { parseAmount, parsePercent } from "./money.ts";
import { LEGACY_KEYS, say } from "./say.ts";
import { t } from "./testing.ts";

// Values for every placeholder the legacy messages take.
const values = {
  written: "USD",
  currency: "EUR",
  input: '"1,234"',
  part: '"ten=8"',
  quantity: "10",
  error: "Too large.",
};

test("the English for callers without a translator is the catalog's, word for word", () => {
  for (const key of LEGACY_KEYS) {
    for (const digits of [0, 1, 2, 3]) {
      assert.equal(say(undefined, key, { ...values, digits }), t(key, { ...values, digits }), key);
    }
  }
});

test("a parse error reads the same with the English translator and without one", () => {
  for (const input of ["", "abc", "-5", "1,234", "12.505", "$12", "99999999999999999999"]) {
    assert.deepEqual(parseAmount(input, 2, "EUR", t), parseAmount(input, 2, "EUR"), input);
  }
  assert.deepEqual(parseAmount("1.5", 0, "JPY", t), parseAmount("1.5", 0, "JPY"));
  for (const input of ["", "abc", "1.234", "-101", "10001", "99999999999999999999"]) {
    assert.deepEqual(parsePercent(input, t), parsePercent(input), input);
  }
  for (const input of ["ten=8", "10=8.505"]) {
    assert.deepEqual(parseBreaks(input, 2, "EUR", t), parseBreaks(input, 2, "EUR"), input);
  }
});

test("counts are plurals, written for the language", () => {
  assert.equal(breaksLabel(t, 1), "1 break");
  assert.equal(breaksLabel(t, 1234), "1,234 breaks");
  assert.equal(t("price_lists.form_too_many_products", { max: 1000 }), "At most 1,000 products.");
});
