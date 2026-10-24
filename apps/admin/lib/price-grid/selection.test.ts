import assert from "node:assert/strict";
import { test } from "node:test";

import {
  cellsOf,
  contains,
  extendTo,
  moveBy,
  nextEditableInColumn,
  rectOf,
  sameCell,
  selectAll,
  selectCell,
  tabFrom,
  tabToEditable,
} from "./selection.ts";

const size = { rows: 4, cols: 3 };

test("a rectangle spans anchor and focus whichever way it was drawn", () => {
  const selection = extendTo(selectCell({ row: 3, col: 2 }), { row: 1, col: 0 });
  assert.deepEqual(rectOf(selection), { top: 1, left: 0, bottom: 3, right: 2 });
  assert.equal(contains(rectOf(selection), { row: 2, col: 1 }), true);
  assert.equal(contains(rectOf(selection), { row: 0, col: 1 }), false);
});

test("arrows move and collapse; with Shift they extend from the anchor", () => {
  const start = selectCell({ row: 1, col: 1 });
  assert.deepEqual(moveBy(start, 1, 0, size, false), selectCell({ row: 2, col: 1 }));
  assert.deepEqual(moveBy(start, 1, 1, size, true), {
    anchor: { row: 1, col: 1 },
    focus: { row: 2, col: 2 },
  });
});

test("moving stops at the grid's edges", () => {
  assert.deepEqual(moveBy(selectCell({ row: 0, col: 0 }), -1, -1, size, false).focus, {
    row: 0,
    col: 0,
  });
  assert.deepEqual(moveBy(selectCell({ row: 3, col: 2 }), 5, 5, size, false).focus, {
    row: 3,
    col: 2,
  });
});

test("tab wraps onto the next row and back, and stops at the ends", () => {
  assert.deepEqual(tabFrom({ row: 0, col: 2 }, size, false), { row: 1, col: 0 });
  assert.deepEqual(tabFrom({ row: 1, col: 0 }, size, true), { row: 0, col: 2 });
  assert.deepEqual(tabFrom({ row: 3, col: 2 }, size, false), { row: 3, col: 2 });
  assert.deepEqual(tabFrom({ row: 0, col: 0 }, size, true), { row: 0, col: 0 });
});

test("cells of a rectangle, row by row", () => {
  assert.deepEqual(cellsOf({ top: 0, left: 1, bottom: 1, right: 2 }), [
    { row: 0, col: 1 },
    { row: 0, col: 2 },
    { row: 1, col: 1 },
    { row: 1, col: 2 },
  ]);
});

test("select all covers the grid", () => {
  assert.deepEqual(rectOf(selectAll(size)), { top: 0, left: 0, bottom: 3, right: 2 });
});

// Rows 0 and 2 are product headers; only column 2 takes prices.
const editable = ({ row, col }: { row: number; col: number }) =>
  row !== 0 && row !== 2 && col === 2;

test("enter moves to the next editable cell down the column, past product headers", () => {
  assert.deepEqual(nextEditableInColumn({ row: 1, col: 2 }, 1, size, editable), {
    row: 3,
    col: 2,
  });
  assert.deepEqual(nextEditableInColumn({ row: 3, col: 2 }, -1, size, editable), {
    row: 1,
    col: 2,
  });
});

test("enter stays put when no editable cell is left that way", () => {
  assert.deepEqual(nextEditableInColumn({ row: 3, col: 2 }, 1, size, editable), {
    row: 3,
    col: 2,
  });
  assert.deepEqual(nextEditableInColumn({ row: 1, col: 2 }, -1, size, editable), {
    row: 1,
    col: 2,
  });
});

test("tab skips names, read-only prices and headers to the next editable cell", () => {
  assert.deepEqual(tabToEditable({ row: 1, col: 2 }, size, false, editable), { row: 3, col: 2 });
  assert.deepEqual(tabToEditable({ row: 3, col: 2 }, size, true, editable), { row: 1, col: 2 });
  assert.deepEqual(tabToEditable({ row: 0, col: 0 }, size, false, editable), { row: 1, col: 2 });
});

test("tab past the last editable cell stays, so focus can leave the grid", () => {
  assert.deepEqual(tabToEditable({ row: 3, col: 2 }, size, false, editable), { row: 3, col: 2 });
  assert.deepEqual(tabToEditable({ row: 1, col: 2 }, size, true, editable), { row: 1, col: 2 });
});

test("same cell compares rows and columns", () => {
  assert.equal(sameCell({ row: 1, col: 2 }, { row: 1, col: 2 }), true);
  assert.equal(sameCell({ row: 1, col: 2 }, { row: 2, col: 1 }), false);
});
