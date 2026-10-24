import assert from "node:assert/strict";
import { test } from "node:test";

import { gridCommand, isSaveKey } from "./keys.ts";

const press = (key: string, mods: { shift?: boolean; mod?: boolean; alt?: boolean } = {}) =>
  gridCommand({ key, shift: false, mod: false, alt: false, ...mods });

test("arrows move by one cell, Shift extends", () => {
  assert.deepEqual(press("ArrowDown"), { kind: "move", rows: 1, cols: 0, extend: false });
  assert.deepEqual(press("ArrowLeft", { shift: true }), {
    kind: "move",
    rows: 0,
    cols: -1,
    extend: true,
  });
});

test("Ctrl/Cmd with an arrow goes to the grid's edge", () => {
  const command = press("ArrowUp", { mod: true });
  assert.equal(command?.kind, "move");
  assert.ok(command?.kind === "move" && command.rows < -1_000_000);
});

test("Home and End go along the row; with Ctrl/Cmd to the grid's corners", () => {
  const home = press("Home");
  assert.ok(home?.kind === "move" && home.rows === 0 && home.cols < 0);
  const end = press("End", { mod: true });
  assert.ok(end?.kind === "move" && end.rows > 0 && end.cols > 0);
});

test("Enter and F2 edit in place; Tab and Shift+Tab move along", () => {
  assert.deepEqual(press("Enter"), { kind: "edit" });
  assert.deepEqual(press("F2"), { kind: "edit" });
  assert.deepEqual(press("Tab"), { kind: "tab", backwards: false });
  assert.deepEqual(press("Tab", { shift: true }), { kind: "tab", backwards: true });
});

test("typing a character starts editing with it", () => {
  assert.deepEqual(press("7"), { kind: "type", text: "7" });
  assert.deepEqual(press(","), { kind: "type", text: "," });
  assert.deepEqual(press("A", { shift: true }), { kind: "type", text: "A" });
});

test("modifier chords and named keys don't type", () => {
  assert.equal(press("c", { mod: true }), null);
  assert.equal(press("v", { mod: true }), null);
  assert.equal(press("5", { alt: true }), null);
  assert.equal(press("Shift"), null);
  assert.equal(press("PageDown"), null);
});

test("Delete and Backspace clear, Escape collapses the selection", () => {
  assert.deepEqual(press("Delete"), { kind: "clear" });
  assert.deepEqual(press("Backspace"), { kind: "clear" });
  assert.deepEqual(press("Escape"), { kind: "collapse" });
});

test("Ctrl/Cmd+D fills down, +A selects all, +S saves", () => {
  assert.deepEqual(press("d", { mod: true }), { kind: "fillDown" });
  assert.deepEqual(press("A", { mod: true, shift: true }), { kind: "selectAll" });
  assert.deepEqual(press("s", { mod: true }), { kind: "save" });
});

test("the save key is Ctrl/Cmd+S alone", () => {
  assert.equal(isSaveKey({ key: "s", shift: false, mod: true, alt: false }), true);
  assert.equal(isSaveKey({ key: "S", shift: true, mod: true, alt: false }), true);
  assert.equal(isSaveKey({ key: "s", shift: false, mod: false, alt: false }), false);
  assert.equal(isSaveKey({ key: "s", shift: false, mod: true, alt: true }), false);
});
