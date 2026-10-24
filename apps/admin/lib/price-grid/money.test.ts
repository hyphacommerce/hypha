import assert from "node:assert/strict";
import { test } from "node:test";

import { MAX_AMOUNT, adjustAmount, formatAmount, parseAmount, parsePercent } from "./money.ts";

const amount = (input: string, digits = 2) => {
  const parsed = parseAmount(input, digits);
  return parsed.ok ? parsed.value : parsed.error;
};

test("a decimal point or a decimal comma", () => {
  assert.equal(amount("12.5"), 1250);
  assert.equal(amount("12,50"), 1250);
  assert.equal(amount("12"), 1200);
  assert.equal(amount(".5"), 50);
  assert.equal(amount("0,05"), 5);
});

test("thousands grouped by spaces, apostrophes, commas or points", () => {
  assert.equal(amount("1 234,50"), 123_450);
  assert.equal(amount("1 234,50"), 123_450);
  assert.equal(amount("1'234.50"), 123_450);
  assert.equal(amount("1,234.50"), 123_450);
  assert.equal(amount("1.234,50"), 123_450);
  assert.equal(amount("1,234,567"), 123_456_700);
  assert.equal(amount("1.234.567,5"), 123_456_750);
});

test("one separator before three digits the currency doesn't have is ambiguous", () => {
  assert.equal(
    amount("1.234"),
    '"1.234" is ambiguous: write it without the separator, or with 2 decimals.',
  );
  assert.match(String(amount("12,345")), /ambiguous/);
});

test("three digits after one separator are decimals when the currency has three", () => {
  assert.equal(amount("1.234", 3), 1234);
  assert.equal(amount("1,234", 3), 1234);
});

test("a leading zero never groups", () => {
  assert.equal(amount("0,500"), "At most 2 decimal places.");
  assert.equal(amount("0,500", 3), 500);
});

test("a currency's code or sign at either end is dropped", () => {
  assert.equal(amount("EUR 12.50"), 1250);
  assert.equal(amount("12,50 €"), 1250);
  assert.equal(amount("$12"), 1200);
});

test("more decimals than the currency has are refused, not rounded", () => {
  assert.equal(amount("12.3456"), "At most 2 decimal places.");
  assert.equal(amount("12.5", 0), "This currency has no decimals.");
  assert.equal(amount("1234", 0), 1234);
});

test("garbage, negatives and nothing are refused with a reason", () => {
  assert.equal(amount(""), "Enter an amount.");
  assert.equal(amount("  "), "Enter an amount.");
  assert.equal(amount("-5"), "Can't be negative.");
  assert.equal(amount("abc"), '"abc" isn\'t an amount.');
  assert.equal(amount("12.50.1"), '"12.50.1" isn\'t an amount.');
  assert.equal(amount("1,23,4.00"), '"1,23,4.00" isn\'t an amount.');
  assert.equal(amount("1.2,3.4"), '"1.2,3.4" isn\'t an amount.');
  assert.equal(amount("12x"), '"12x" isn\'t an amount.');
});

test("the largest amount is the API's", () => {
  assert.equal(amount("90071992547409.91"), MAX_AMOUNT);
  assert.equal(amount("90071992547409.92"), "Too large.");
});

test("amounts format as plain decimals in the currency's digits", () => {
  assert.equal(formatAmount(1250, 2), "12.50");
  assert.equal(formatAmount(5, 2), "0.05");
  assert.equal(formatAmount(0, 2), "0.00");
  assert.equal(formatAmount(1234, 0), "1234");
  assert.equal(formatAmount(1234, 3), "1.234");
  assert.equal(formatAmount(-5, 2), "-0.05");
  assert.equal(formatAmount(MAX_AMOUNT, 2), "90071992547409.91");
});

test("adjusting rounds half away from zero, as Money::adjust", () => {
  assert.equal(adjustAmount(1000, -1000), 900);
  assert.equal(adjustAmount(1005, -1000), 905); // 904.5
  assert.equal(adjustAmount(1006, -1000), 905); // 905.4
  assert.equal(adjustAmount(1001, -1000), 901); // 900.9
  assert.equal(adjustAmount(1000, 500), 1050);
  assert.equal(adjustAmount(-1005, -1000), -905); // -904.5
  assert.equal(adjustAmount(1000, -10_000), 0);
});

test("adjusting past the largest amount is null", () => {
  assert.equal(adjustAmount(MAX_AMOUNT, 0), MAX_AMOUNT);
  assert.equal(adjustAmount(MAX_AMOUNT, 1), null);
});

const bps = (input: string) => {
  const parsed = parsePercent(input);
  return parsed.ok ? parsed.value : parsed.error;
};

test("percentages become basis points", () => {
  assert.equal(bps("-10"), -1000);
  assert.equal(bps("+5.5"), 550);
  assert.equal(bps("12,25%"), 1225);
  assert.equal(bps("−20"), -2000);
  assert.equal(bps("-100"), -10_000);
  assert.equal(bps("-100.01"), "Can't take off more than 100%.");
  assert.equal(bps("1.234"), "At most two decimal places.");
  assert.equal(bps("ten"), "A percentage, like -10 or 5.5.");
});
