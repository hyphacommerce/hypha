import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createFormat,
  formatBytes,
  formatDateTime,
  formatMoney,
  formatUnitCost,
  parseUnitCost,
  unitCostInput,
  unitCostTotal,
} from "./format.ts";
import { localeOf } from "./pick-language.ts";

const currencies = { EUR: 2, JPY: 0, KWD: 3, USD: 2 };

test("English keeps the formats the admin always had", () => {
  const format = createFormat(localeOf("en"), { currencies, timeZone: "Europe/Zagreb" });
  assert.equal(format.money({ amount: 1800, currency: "EUR" }), "€18.00");
  assert.equal(format.money({ amount: 1800, currency: "USD" }), "US$18.00");
  assert.equal(format.money({ amount: 1800, currency: "JPY" }), "JP¥1,800");
  assert.equal(format.unitCost({ amount: "412.5", currency: "EUR" }), "€4.125");
  assert.equal(format.number(1234.5), "1,234.5");
  assert.equal(format.dateTime("2026-10-15T07:30:00Z"), "15 Oct 2026, 09:30 CEST");
  assert.equal(format.dateTime("2020-10-15T07:30:00Z", "short"), "15 Oct 2020");
  assert.equal(format.date("2026-11-11T12:00:00Z"), "11 Nov 2026");
  assert.equal(format.date("2026-11-11T12:00:00Z", "short"), "11 Nov");
  assert.equal(format.bytes(980), "980 B");
  assert.equal(format.bytes(1024 * 1024 * 1.4), "1.4 MB");
});

test("the functions without a language are English", () => {
  assert.equal(formatMoney({ amount: 1800, currency: "EUR" }, currencies), "€18.00");
  assert.equal(formatDateTime("2026-10-15T07:30:00Z", "UTC"), "15 Oct 2026, 07:30 UTC");
  assert.equal(formatBytes(1536), "1.5 KB");
});

test("the pseudo-language formats as English", () => {
  const format = createFormat(localeOf("en-XA"), { currencies });
  assert.equal(format.money({ amount: 1800, currency: "EUR" }), "€18.00");
  assert.equal(format.date("2026-11-11T12:00:00Z"), "11 Nov 2026");
});

test("another language formats its own way", () => {
  const format = createFormat("de", { currencies, timeZone: "Europe/Berlin" });
  assert.equal(format.number(1234.5), "1.234,5");
  assert.equal(format.money({ amount: 123450, currency: "EUR" }), "1.234,50 €");
  assert.equal(format.percent(0.125), "12,5 %");
  assert.equal(format.bytes(1536), "1,5 KB");
  assert.equal(format.date("2026-03-11T12:00:00Z"), "11. März 2026");
  assert.equal(format.list(["A", "B", "C"]), "A, B und C");
  assert.equal(format.list(["A", "B", "C"], "disjunction"), "A, B oder C");
  assert.equal(format.countryName("HR"), "Kroatien");
  assert.equal(format.currencyName("EUR"), "Euro");
  assert.equal(format.languageName("sr-Latn"), "Serbisch (Lateinisch)");
  assert.deepEqual(
    ["z", "ä", "a"].toSorted((a, b) => format.compare(a, b)),
    ["a", "ä", "z"],
  );
});

test("names Intl doesn't have fall back to the code", () => {
  const format = createFormat(localeOf("en"));
  assert.equal(format.countryName("not a code"), "not a code");
  assert.equal(format.currencyName("not a code"), undefined);
});

test("plural categories follow the language", () => {
  assert.equal(createFormat(localeOf("en")).plural(1), "one");
  assert.equal(createFormat("sr-Latn").plural(3), "few");
});

test("money in a currency the store's table lacks throws rather than guess", () => {
  assert.throws(
    () => createFormat(localeOf("en")).money({ amount: 1, currency: "EUR" }),
    /unknown currency/,
  );
});

test("a time that isn't one is a dash", () => {
  assert.equal(createFormat(localeOf("en")).dateTime("nonsense"), "—");
  assert.equal(createFormat(localeOf("en")).date("nonsense"), "—");
});

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
