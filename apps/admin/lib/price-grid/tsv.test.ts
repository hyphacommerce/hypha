import assert from "node:assert/strict";
import { test } from "node:test";

import { pastePlacement, parseTsv, toTsv } from "./tsv.ts";

test("rows and cells from a spreadsheet's copy, with its trailing line break", () => {
  assert.deepEqual(parseTsv("1\t2\r\n3\t4\r\n"), [
    ["1", "2"],
    ["3", "4"],
  ]);
  assert.deepEqual(parseTsv("12.50"), [["12.50"]]);
  assert.deepEqual(parseTsv(""), []);
});

test("empty cells are kept", () => {
  assert.deepEqual(parseTsv("1\t\t3\n\t\t"), [
    ["1", "", "3"],
    ["", "", ""],
  ]);
});

test("quoted cells hold tabs, new lines and doubled quotes", () => {
  assert.deepEqual(parseTsv('"a\tb"\t"line\nbreak"\t"say ""hi"""'), [
    ["a\tb", "line\nbreak", 'say "hi"'],
  ]);
});

test("copying quotes only what needs it, and round-trips", () => {
  const rows = [
    ["12.50", "a\tb"],
    ['say "hi"', ""],
  ];
  const text = toTsv(rows);
  assert.equal(text, '12.50\t"a\tb"\n"say ""hi"""\t');
  assert.deepEqual(parseTsv(text), rows);
});

test("a pasted block lands at the selection's corner, cut at the grid's edge", () => {
  const placed = pastePlacement(
    { top: 2, left: 1, bottom: 2, right: 1 },
    [
      ["a", "b"],
      ["c", "d"],
    ],
    { rows: 3, cols: 3 },
  );
  assert.deepEqual(placed, [
    { row: 2, col: 1, text: "a" },
    { row: 2, col: 2, text: "b" },
  ]);
});

test("one value fills the whole selection", () => {
  const placed = pastePlacement({ top: 0, left: 0, bottom: 1, right: 1 }, [["9"]], {
    rows: 5,
    cols: 5,
  });
  assert.equal(placed.length, 4);
  assert.ok(placed.every((p) => p.text === "9"));
});

test("a block repeats when its size divides the selection's", () => {
  const placed = pastePlacement({ top: 0, left: 0, bottom: 3, right: 0 }, [["a"], ["b"]], {
    rows: 5,
    cols: 5,
  });
  assert.deepEqual(
    placed.map((p) => p.text),
    ["a", "b", "a", "b"],
  );
});

test("a block that doesn't divide the selection is pasted once", () => {
  const placed = pastePlacement({ top: 0, left: 0, bottom: 2, right: 0 }, [["a"], ["b"]], {
    rows: 5,
    cols: 5,
  });
  assert.deepEqual(
    placed.map((p) => p.text),
    ["a", "b"],
  );
});

test("ragged rows paste their missing cells as empty", () => {
  const placed = pastePlacement({ top: 0, left: 0, bottom: 0, right: 0 }, [["a", "b"], ["c"]], {
    rows: 5,
    cols: 5,
  });
  assert.deepEqual(
    placed.map((p) => p.text),
    ["a", "b", "c", ""],
  );
});
