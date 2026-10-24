// What a key does in the grid while no cell is being edited, as a spreadsheet does it.
// The component reads the event into a `KeyInput` and acts on the command; kept apart
// so the keyboard model is tested without a browser.

export type KeyInput = {
  key: string;
  shift: boolean;
  /** Ctrl, or Cmd on a Mac. */
  mod: boolean;
  alt: boolean;
};

export type GridCommand =
  /** Arrows, Home/End and Ctrl/Cmd with them; Shift extends the selection. */
  | { kind: "move"; rows: number; cols: number; extend: boolean }
  /** Tab and Shift+Tab: the next (or previous) editable cell. */
  | { kind: "tab"; backwards: boolean }
  /** Enter or F2: edit the cell keeping its text, open its breaks, fold its product. */
  | { kind: "edit" }
  /** A printable key: start editing, the cell's text replaced by it. */
  | { kind: "type"; text: string }
  /** Space: folds a product; elsewhere it types. */
  | { kind: "space" }
  /** Escape: back to the one cell under the cursor. */
  | { kind: "collapse" }
  /** Delete or Backspace: empty the selected cells. */
  | { kind: "clear" }
  | { kind: "fillDown" }
  | { kind: "selectAll" }
  | { kind: "save" };

/** As far as the grid goes: `moveBy` stops at its edges. */
const FAR = Number.MAX_SAFE_INTEGER;

/** The command for a key pressed on the grid, or null when the grid leaves it be. */
export function gridCommand({ key, shift, mod, alt }: KeyInput): GridCommand | null {
  const move = (rows: number, cols: number): GridCommand => ({
    kind: "move",
    rows,
    cols,
    extend: shift,
  });
  // Ctrl/Cmd with an arrow goes to that edge of the grid, as in a spreadsheet.
  const by = mod ? FAR : 1;
  switch (key) {
    case "ArrowUp":
      return move(-by, 0);
    case "ArrowDown":
      return move(by, 0);
    case "ArrowLeft":
      return move(0, -by);
    case "ArrowRight":
      return move(0, by);
    case "Home":
      return mod ? move(-FAR, -FAR) : move(0, -FAR);
    case "End":
      return mod ? move(FAR, FAR) : move(0, FAR);
    case "Tab":
      return { kind: "tab", backwards: shift };
    case "Enter":
    case "F2":
      return { kind: "edit" };
    case "Escape":
      return { kind: "collapse" };
    case "Delete":
    case "Backspace":
      return { kind: "clear" };
    case " ":
      return mod || alt ? null : { kind: "space" };
  }
  if (mod && !alt) {
    switch (key.toLowerCase()) {
      case "d":
        return { kind: "fillDown" };
      case "a":
        return { kind: "selectAll" };
      case "s":
        return { kind: "save" };
    }
    return null;
  }
  if (key.length === 1 && !alt) return { kind: "type", text: key };
  return null;
}

/** Ctrl/Cmd+S, which saves wherever focus is on the page, an open cell included. */
export function isSaveKey({ key, mod, alt }: KeyInput): boolean {
  return mod && !alt && key.toLowerCase() === "s";
}
