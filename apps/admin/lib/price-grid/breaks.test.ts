import assert from "node:assert/strict";
import { test } from "node:test";

import { breakProblems, breaksLabel, formatBreaks, parseBreaks } from "./breaks.ts";

test("breaks round-trip through their cell text", () => {
  const breaks = [
    { min_quantity: 10, amount: 800 },
    { min_quantity: 20, amount: 750 },
  ];
  const text = formatBreaks(breaks, 2);
  assert.equal(text, "10=8.00; 20=7.50");
  assert.deepEqual(parseBreaks(text, 2, "EUR"), { ok: true, value: breaks });
});

test("breaks parse leniently: other joiners, new lines, decimal commas", () => {
  assert.deepEqual(parseBreaks("10 @ 8,00\n20: 7.5;", 2, "EUR"), {
    ok: true,
    value: [
      { min_quantity: 10, amount: 800 },
      { min_quantity: 20, amount: 750 },
    ],
  });
  assert.deepEqual(parseBreaks("  ", 2, "EUR"), { ok: true, value: [] });
});

test("a part that isn't a break, or a bad price, is refused with the part", () => {
  assert.deepEqual(parseBreaks("ten=8", 2, "EUR"), {
    ok: false,
    error: '"ten=8" isn\'t a break, like 10=8.00.',
  });
  assert.deepEqual(parseBreaks("10=8.0001", 2, "EUR"), {
    ok: false,
    error: "Break 10: At most 2 decimal places.",
  });
});

test("good breaks have no problems", () => {
  assert.deepEqual(
    breakProblems(
      [
        { min_quantity: 2, amount: 900 },
        { min_quantity: 10, amount: 800 },
      ],
      1000,
    ),
    [],
  );
  assert.deepEqual(breakProblems([], null), []);
});

test("breaks need a fixed price", () => {
  assert.deepEqual(breakProblems([{ min_quantity: 2, amount: 900 }], null), [
    "Breaks need a fixed price.",
  ]);
});

test("quantities start at 2 and increase strictly", () => {
  assert.deepEqual(breakProblems([{ min_quantity: 1, amount: 900 }], 1000), [
    "Quantities start at 2 (1).",
  ]);
  assert.deepEqual(
    breakProblems(
      [
        { min_quantity: 5, amount: 900 },
        { min_quantity: 5, amount: 800 },
      ],
      1000,
    ),
    ["Quantities must increase (5 after 5)."],
  );
});

test("each price is below the fixed price and the one before it", () => {
  assert.deepEqual(breakProblems([{ min_quantity: 2, amount: 1000 }], 1000), [
    "The price from 2 must be below the fixed price.",
  ]);
  assert.deepEqual(
    breakProblems(
      [
        { min_quantity: 2, amount: 900 },
        { min_quantity: 5, amount: 900 },
      ],
      1000,
    ),
    ["The price from 5 must be below the one before it."],
  );
});

test("at most ten breaks", () => {
  const eleven = Array.from({ length: 11 }, (_, i) => ({
    min_quantity: i + 2,
    amount: 1000 - i - 1,
  }));
  assert.deepEqual(breakProblems(eleven, 1000), ["At most 10 breaks."]);
  assert.deepEqual(breakProblems(eleven.slice(0, 10), 1000), []);
});

test("labels count breaks", () => {
  assert.equal(breaksLabel(1), "1 break");
  assert.equal(breaksLabel(3), "3 breaks");
});
