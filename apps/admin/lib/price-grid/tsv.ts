// Tab-separated text, the clipboard's format between spreadsheets: what Excel, Numbers
// and Google Sheets put there on copy, and what they take on paste. A cell holding a tab,
// a new line or a quote is quoted, its quotes doubled.

import { type Rect, rectHeight, rectWidth } from "./selection.ts";

/** Rows of cells from clipboard text. A trailing line break ends the last row. */
export function parseTsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  let atCellStart = true;
  const input = text.replaceAll("\r\n", "\n").replaceAll("\r", "\n");

  for (let i = 0; i < input.length; i++) {
    const char = input.charAt(i);
    if (quoted) {
      if (char === '"' && input.charAt(i + 1) === '"') {
        cell += '"';
        i++;
      } else if (char === '"') {
        quoted = false;
      } else {
        cell += char;
      }
    } else if (char === '"' && atCellStart) {
      quoted = true;
      atCellStart = false;
    } else if (char === "\t") {
      row.push(cell);
      cell = "";
      atCellStart = true;
    } else if (char === "\n") {
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
      atCellStart = true;
    } else {
      cell += char;
      atCellStart = false;
    }
  }
  if (!atCellStart || cell !== "" || row.length > 0) {
    row.push(cell);
    rows.push(row);
  }
  return rows;
}

function quote(cell: string): string {
  return /[\t\n\r"]/.test(cell) ? `"${cell.replaceAll('"', '""')}"` : cell;
}

/** Clipboard text from rows of cells. */
export function toTsv(rows: readonly (readonly string[])[]): string {
  return rows.map((row) => row.map(quote).join("\t")).join("\n");
}

export type Placed = { row: number; col: number; text: string };

/**
 * Where pasted cells land, as a spreadsheet places them: at the selection's top-left
 * corner, as many rows and columns as were copied (cut at the grid's edge). A single
 * value fills the whole selection, and a block whose size divides the selection's is
 * repeated to fill it.
 */
export function pastePlacement(
  rect: Rect,
  data: readonly (readonly string[])[],
  size: { rows: number; cols: number },
): Placed[] {
  const height = data.length;
  const width = Math.max(0, ...data.map((row) => row.length));
  if (height === 0 || width === 0) return [];

  const tiles =
    rectHeight(rect) % height === 0 && rectWidth(rect) % width === 0
      ? { rows: rectHeight(rect), cols: rectWidth(rect) }
      : { rows: height, cols: width };

  const placed: Placed[] = [];
  for (let r = 0; r < tiles.rows; r++) {
    const row = rect.top + r;
    if (row >= size.rows) break;
    for (let c = 0; c < tiles.cols; c++) {
      const col = rect.left + c;
      if (col >= size.cols) break;
      placed.push({ row, col, text: data[r % height]?.[c % width] ?? "" });
    }
  }
  return placed;
}
