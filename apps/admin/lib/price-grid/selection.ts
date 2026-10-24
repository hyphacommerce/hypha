// The grid's selection: a rectangle between the cell where it started (the anchor) and
// the one the keyboard or mouse is on (the focus), as in a spreadsheet. Plain values, so
// React state holds them as they are.

export type Cell = { row: number; col: number };

export type Selection = { anchor: Cell; focus: Cell };

/** Inclusive bounds. */
export type Rect = { top: number; left: number; bottom: number; right: number };

/** How many rows and columns the grid has. */
export type Size = { rows: number; cols: number };

export function selectCell(cell: Cell): Selection {
  return { anchor: cell, focus: cell };
}

/** From the anchor to `cell`: Shift+click. */
export function extendTo(selection: Selection, cell: Cell): Selection {
  return { anchor: selection.anchor, focus: cell };
}

export function rectOf({ anchor, focus }: Selection): Rect {
  return {
    top: Math.min(anchor.row, focus.row),
    left: Math.min(anchor.col, focus.col),
    bottom: Math.max(anchor.row, focus.row),
    right: Math.max(anchor.col, focus.col),
  };
}

export function rectHeight(rect: Rect): number {
  return rect.bottom - rect.top + 1;
}

export function rectWidth(rect: Rect): number {
  return rect.right - rect.left + 1;
}

export function contains(rect: Rect, { row, col }: Cell): boolean {
  return row >= rect.top && row <= rect.bottom && col >= rect.left && col <= rect.right;
}

/** Every cell of `rect`, row by row. */
export function cellsOf(rect: Rect): Cell[] {
  const cells: Cell[] = [];
  for (let row = rect.top; row <= rect.bottom; row++) {
    for (let col = rect.left; col <= rect.right; col++) cells.push({ row, col });
  }
  return cells;
}

function clamp(value: number, max: number): number {
  return Math.min(Math.max(value, 0), Math.max(max - 1, 0));
}

/**
 * The arrow keys: the focus moves by `rows`/`cols`, held inside the grid. With `extend`
 * (Shift) the anchor stays and the rectangle grows or shrinks; without it the selection
 * collapses onto the new cell.
 */
export function moveBy(
  selection: Selection,
  rows: number,
  cols: number,
  size: Size,
  extend: boolean,
): Selection {
  const focus = {
    row: clamp(selection.focus.row + rows, size.rows),
    col: clamp(selection.focus.col + cols, size.cols),
  };
  return extend ? { anchor: selection.anchor, focus } : selectCell(focus);
}

/** Tab and Shift+Tab: the next cell along the row, wrapping onto the next row. */
export function tabFrom(cell: Cell, size: Size, backwards: boolean): Cell {
  const index = cell.row * size.cols + cell.col + (backwards ? -1 : 1);
  const last = size.rows * size.cols - 1;
  const at = Math.min(Math.max(index, 0), last);
  return { row: Math.floor(at / size.cols), col: at % size.cols };
}

/** Ctrl/Cmd+A: everything. */
export function selectAll(size: Size): Selection {
  return {
    anchor: { row: 0, col: 0 },
    focus: { row: Math.max(size.rows - 1, 0), col: Math.max(size.cols - 1, 0) },
  };
}
