"use client";

/**
 * A price list's prices as a spreadsheet: products with their variants beneath, a
 * column per price, edited in place and saved together.
 *
 * Usage:
 *
 *     <PriceGrid
 *       currency={list.currency}
 *       products={rows}                 // PriceGridProduct[]: a page of the catalog
 *       columns={["base_price", "list_price", "fixed_price", "breaks"]}
 *       toolbar={<Filters />}           // category, search, "fixed only": the page's
 *       onSave={(diff) => saveFixedPrices(list.id, diff)}
 *     />
 *
 * - `currency` is the list's; editable cells are in its minor units (`useCurrencies`).
 * - `products` (`lib/price-grid/types.ts`): each variant's base and compare-at prices,
 *   `adjusted_price` (what the list's adjustment makes of the base, null when the
 *   adjustment doesn't cover it) and `fixed` (`{price, compare_at, breaks}` or null).
 *   A product whose only variant has no title (no options) is one row.
 * - `columns` picks and orders the columns (all six by default): `base_price`,
 *   `compare_at_price`, `list_price` (the price the list gives, greyed when it comes
 *   from the adjustment) are read-only; `fixed_price`, `fixed_compare_at` and `breaks`
 *   are edited.
 * - `onSave(diff)` gets a `PriceGridDiff`: `upserts` (`{variant, price, compare_at,
 *   breaks}`, money as `{amount, currency}`, for `PUT …/fixed-prices` in chunks of
 *   1,000) and `clears` (`{variant}`, for `DELETE …/fixed-prices`). It answers
 *   `{error?, rowErrors?}`: nothing means saved and the drafts go (refresh `products`
 *   in the action); `rowErrors` (by variant ID) keeps those variants' drafts with the
 *   message on their row and drops the rest as saved; `error` alone keeps every draft.
 * - `isReadOnly` (no `pricing:write`) shows the prices without editing.
 *
 * Keys: arrows (Shift extends), Tab, Home/End, Enter or F2 to edit (a breaks cell opens
 * its editor; a product's name collapses it), typing replaces, Escape cancels, Delete
 * clears, Ctrl/Cmd+C copies the selection as TSV, Ctrl/Cmd+V pastes a spreadsheet's
 * cells, Ctrl/Cmd+D fills down, Ctrl/Cmd+A selects everything.
 */

import { Button } from "@astryxdesign/core/Button";
import { Text } from "@astryxdesign/core/Text";
import { ChevronDownIcon, ChevronRightIcon } from "@heroicons/react/20/solid";
import {
  type ClipboardEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useMemo,
  useRef,
  useState,
  useTransition,
} from "react";

import { useCurrencies } from "@/components/currencies";
import { SaveBar } from "@/components/save-bar";
import { type Currencies, formatMoney } from "@/lib/format";
import { breaksLabel, parseBreaks } from "@/lib/price-grid/breaks";
import {
  type CellChange,
  type Drafts,
  type Target,
  applyChanges,
  cellKey,
  changesAt,
  clearCells,
  currentDraft,
  fillDown,
  normalizeCell,
  percentOfBase,
  review,
  setTo,
} from "@/lib/price-grid/edits";
import { formatAmount, parseAmount } from "@/lib/price-grid/money";
import { type PriceGridRow, variantsById, visibleRows } from "@/lib/price-grid/rows";
import {
  type Cell,
  type Selection,
  cellsOf,
  contains,
  extendTo,
  moveBy,
  rectOf,
  selectAll,
  selectCell,
  tabFrom,
} from "@/lib/price-grid/selection";
import { pastePlacement, parseTsv, toTsv } from "@/lib/price-grid/tsv";
import {
  type Money,
  type PriceGridColumn,
  type PriceGridDiff,
  type PriceGridProduct,
  type PriceGridVariant,
  isEditableColumn,
} from "@/lib/price-grid/types";

import { BreaksDialog } from "./breaks-dialog";
import { type BulkAction, BulkDialog } from "./bulk-dialog";

export type { PriceGridRow } from "@/lib/price-grid/rows";
export type {
  FixedPrice,
  FixedPriceUpsert,
  Money,
  PriceBreak,
  PriceGridColumn,
  PriceGridDiff,
  PriceGridProduct,
  PriceGridVariant,
} from "@/lib/price-grid/types";

/** What `onSave` answers. */
export type PriceGridSaveResult = {
  /** Why the save failed as a whole (or a summary of the row errors). */
  error?: string;
  /** By variant ID: why that variant's fixed price wasn't saved. */
  rowErrors?: Readonly<Record<string, string>>;
};

export const ALL_COLUMNS: readonly PriceGridColumn[] = [
  "base_price",
  "compare_at_price",
  "list_price",
  "fixed_price",
  "fixed_compare_at",
  "breaks",
];

const HEADERS: Record<PriceGridColumn, string> = {
  base_price: "Base price",
  compare_at_price: "Compare-at",
  list_price: "List price",
  fixed_price: "Fixed price",
  fixed_compare_at: "Fixed compare-at",
  breaks: "Breaks",
};

type Editing = { cell: Cell; text: string; mode: "replace" | "edit" };

type Dialogs =
  | { kind: "breaks"; variant: PriceGridVariant; title: string }
  | { kind: "bulk"; action: BulkAction }
  | null;

/** Column 0 is the names; the rest are `columns`. */
type GridColumn = "name" | PriceGridColumn;

export function PriceGrid({
  currency,
  products,
  columns = ALL_COLUMNS,
  isReadOnly = false,
  toolbar,
  label = "Prices",
  onSave,
}: {
  currency: string;
  products: readonly PriceGridProduct[];
  columns?: readonly PriceGridColumn[];
  isReadOnly?: boolean;
  /** Filters and the like, at the start of the grid's toolbar. */
  toolbar?: ReactNode;
  /** The grid's accessible name. */
  label?: string;
  onSave: (diff: PriceGridDiff) => Promise<PriceGridSaveResult>;
}) {
  const currencies = useCurrencies();
  const digits = currencies[currency] ?? 2;

  const [drafts, setDrafts] = useState<Drafts>(() => new Map());
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [selection, setSelection] = useState<Selection>(() => selectCell({ row: 0, col: 0 }));
  const [editing, setEditing] = useState<Editing | null>(null);
  const [dialog, setDialog] = useState<Dialogs>(null);
  const [notice, setNotice] = useState<string>();
  const [saveError, setSaveError] = useState<string>();
  const [rowErrors, setRowErrors] = useState<Readonly<Record<string, string>>>({});
  const [isPending, startTransition] = useTransition();
  const gridRef = useRef<HTMLTableElement>(null);

  const rows = useMemo(() => visibleRows(products, collapsed), [products, collapsed]);
  const variants = useMemo(() => variantsById(products), [products]);
  const checked = useMemo(
    () => review(variants, drafts, currency, digits),
    [variants, drafts, currency, digits],
  );

  const gridColumns: GridColumn[] = ["name", ...columns];
  const size = { rows: rows.length, cols: gridColumns.length };
  // The selection as it applies now: rows may have collapsed or the page changed.
  const clampCell = (cell: Cell): Cell => ({
    row: Math.min(cell.row, Math.max(size.rows - 1, 0)),
    col: Math.min(cell.col, size.cols - 1),
  });
  const current: Selection = {
    anchor: clampCell(selection.anchor),
    focus: clampCell(selection.focus),
  };
  const rect = rectOf(current);

  const targetAt = ({ row, col }: Cell): Target | null => {
    const r = rows[row];
    const column = gridColumns[col];
    if (isReadOnly || r?.kind !== "variant" || !column || column === "name") return null;
    return isEditableColumn(column) ? { variant: r.variant.variant, column } : null;
  };
  const textAt = (target: Target): string => {
    const variant = variants.get(target.variant);
    return variant ? currentDraft(drafts, variant, digits)[target.column] : "";
  };
  const selectedTargets = cellsOf(rect).flatMap((cell) => {
    const target = targetAt(cell);
    return target ? [target] : [];
  });

  const focusCell = (cell: Cell) =>
    gridRef.current?.querySelector<HTMLElement>(`[data-cell="${cell.row}:${cell.col}"]`)?.focus();

  // A closing dialog hands focus back to what opened it (a toolbar button) after this
  // handler, so the grid takes it on the next frame.
  const refocusAfterDialog = (cell: Cell) => requestAnimationFrame(() => focusCell(cell));

  const select = (next: Selection) => {
    setSelection(next);
    setNotice(undefined);
    focusCell(next.focus);
  };

  const apply = (changes: readonly CellChange[], skipped = 0) => {
    setDrafts((before) => applyChanges(before, variants, changes, digits, currency));
    setNotice(
      skipped > 0
        ? `${skipped} ${skipped === 1 ? "cell" : "cells"} couldn't take a value and kept theirs.`
        : undefined,
    );
  };

  const toggle = (product: string) =>
    setCollapsed((before) => {
      const next = new Set(before);
      if (!next.delete(product)) next.add(product);
      return next;
    });

  /** Enter, F2, a double click: edit the cell, open its breaks, or fold its product. */
  const activate = (cell: Cell, mode: Editing["mode"], text?: string) => {
    const r = rows[cell.row];
    if (r?.kind === "product" && cell.col === 0) {
      toggle(r.product.id);
      return;
    }
    const target = targetAt(cell);
    if (!target || r?.kind !== "variant") return;
    if (target.column === "breaks") {
      setDialog({ kind: "breaks", variant: r.variant, title: rowTitle(r) });
      return;
    }
    setEditing({ cell, text: text ?? textAt(target), mode });
  };

  /** A key ended the edit: keep the text and move on. */
  const commit = (edit: Editing, next: Cell) => {
    const target = targetAt(edit.cell);
    if (target) apply([{ ...target, text: edit.text }]);
    setEditing(null);
    select(selectCell(next));
  };

  const listPrice = (variant: PriceGridVariant): { money: Money; isAdjusted: boolean } | null => {
    const text = currentDraft(drafts, variant, digits).fixed_price;
    if (text.trim() === "") {
      return variant.adjusted_price ? { money: variant.adjusted_price, isAdjusted: true } : null;
    }
    const amount = parseAmount(text, digits, currency);
    return amount.ok ? { money: { amount: amount.value, currency }, isAdjusted: false } : null;
  };

  const copyText = (cell: Cell): string => {
    const r = rows[cell.row];
    const column = gridColumns[cell.col];
    if (!r || !column) return "";
    if (column === "name") return r.kind === "variant" ? rowTitle(r) : r.product.title;
    if (r.kind !== "variant") return "";
    if (isEditableColumn(column)) return textAt({ variant: r.variant.variant, column });
    if (column === "base_price") return plainAmount(r.variant.base_price, currencies);
    if (column === "compare_at_price") return plainAmount(r.variant.compare_at_price, currencies);
    return plainAmount(listPrice(r.variant)?.money ?? null, currencies);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTableElement>) => {
    if (editing || !(e.target instanceof Node) || !e.currentTarget.contains(e.target)) return;
    const mod = e.metaKey || e.ctrlKey;
    const move = (rowsBy: number, colsBy: number) => {
      e.preventDefault();
      select(moveBy(current, rowsBy, colsBy, size, e.shiftKey));
    };
    switch (e.key) {
      case "ArrowUp":
        return move(-1, 0);
      case "ArrowDown":
        return move(1, 0);
      case "ArrowLeft":
        return move(0, -1);
      case "ArrowRight":
        return move(0, 1);
      case "Home":
        return move(0, -size.cols);
      case "End":
        return move(0, size.cols);
      case "Tab": {
        const next = tabFrom(current.focus, size, e.shiftKey);
        // At either end, Tab leaves the grid as it would any other control.
        if (next.row === current.focus.row && next.col === current.focus.col) return;
        e.preventDefault();
        return select(selectCell(next));
      }
      case "Enter":
      case "F2":
        e.preventDefault();
        return activate(current.focus, "edit");
      case " ":
        if (rows[current.focus.row]?.kind === "product" && current.focus.col === 0) {
          e.preventDefault();
          return activate(current.focus, "edit");
        }
        break;
      case "Escape":
        return select(selectCell(current.focus));
      case "Delete":
      case "Backspace":
        e.preventDefault();
        return apply(clearCells(selectedTargets));
    }
    if (mod && e.key.toLowerCase() === "d") {
      e.preventDefault();
      return apply(fillDown(rect, targetAt, textAt));
    }
    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      return select(selectAll(size));
    }
    // Typing on a money cell starts over with what's typed, as in a spreadsheet.
    if (e.key.length === 1 && !mod && !e.altKey) {
      const target = targetAt(current.focus);
      if (target && target.column !== "breaks") {
        e.preventDefault();
        activate(current.focus, "replace", e.key);
      }
    }
  };

  const onCopy = (e: ClipboardEvent<HTMLTableElement>) => {
    if (editing) return;
    e.preventDefault();
    const lines: string[][] = [];
    for (let row = rect.top; row <= rect.bottom; row++) {
      const line: string[] = [];
      for (let col = rect.left; col <= rect.right; col++) line.push(copyText({ row, col }));
      lines.push(line);
    }
    e.clipboardData.setData("text/plain", toTsv(lines));
  };

  const onPaste = (e: ClipboardEvent<HTMLTableElement>) => {
    if (editing || isReadOnly) return;
    e.preventDefault();
    const data = parseTsv(e.clipboardData.getData("text/plain"));
    const { changes, skipped } = changesAt(pastePlacement(rect, data, size), targetAt);
    apply(changes, skipped);
  };

  const onCellMouseDown = (e: MouseEvent, cell: Cell) => {
    // A click into the cell being edited is the input's; leaving it commits on blur.
    if (e.button !== 0 || (editing?.cell.row === cell.row && editing.cell.col === cell.col)) {
      return;
    }
    if (e.shiftKey) {
      // Keep the browser from selecting the page's text between the two cells.
      e.preventDefault();
      setSelection(extendTo(current, cell));
      focusCell(cell);
    } else {
      setSelection(selectCell(cell));
    }
    setNotice(undefined);
  };

  const onCellMouseEnter = (e: MouseEvent, cell: Cell) => {
    // Dragging with the button held draws the rectangle.
    if (e.buttons & 1 && !editing) setSelection(extendTo(current, cell));
  };

  const save = () =>
    startTransition(async () => {
      // Typing goes on while the save runs. Drafts are replaced, never mutated, so a draft
      // that's still the one sent is what was saved; any other was typed meanwhile.
      const sent = drafts;
      const result = await onSave(checked.diff);
      const failed = result.rowErrors ?? {};
      const failedIds = Object.keys(failed);
      setRowErrors(failed);
      if (result.error && failedIds.length === 0) {
        setSaveError(result.error);
        return;
      }
      setDrafts(
        (before) =>
          new Map(
            [...before].filter(([id, d]) => d !== sent.get(id) || failedIds.includes(id)),
          ),
      );
      setSaveError(
        failedIds.length > 0
          ? (result.error ??
              `${failedIds.length} ${failedIds.length === 1 ? "variant wasn't" : "variants weren't"} saved: see the rows marked.`)
          : undefined,
      );
    });

  const discard = () => {
    setDrafts(new Map());
    setRowErrors({});
    setSaveError(undefined);
    setEditing(null);
  };

  const focusedTarget = targetAt(current.focus);
  const focusedRow = rows[current.focus.row];
  const status =
    (focusedTarget && checked.errors.get(cellKey(focusedTarget.variant, focusedTarget.column))) ??
    (focusedRow?.kind === "variant" ? rowErrors[focusedRow.variant.variant] : undefined) ??
    notice;
  const changeCount = checked.changedVariants;
  const hasMoneyTargets = selectedTargets.some((t) => t.column !== "breaks");

  return (
    <div className="pg">
      <SaveBar
        isDirty={changeCount > 0}
        isPending={isPending}
        error={saveError}
        canSave={checked.errors.size === 0}
        onSave={save}
        onDiscard={discard}
      />
      <div className="pg-card">
        <div className="pg-toolbar">
          {toolbar}
          <div className="pg-spacer" />
          <Text type="supporting" color="secondary">
            {changeCount === 0
              ? "No changes"
              : `${changeCount} ${changeCount === 1 ? "variant" : "variants"} changed${
                  checked.errors.size > 0 ? `, ${checked.errors.size} to fix` : ""
                }`}
          </Text>
          {!isReadOnly && (
            <>
              <Button
                label="Set to…"
                size="sm"
                isDisabled={!hasMoneyTargets}
                onClick={() => setDialog({ kind: "bulk", action: "amount" })}
              />
              <Button
                label="% of base…"
                size="sm"
                isDisabled={!hasMoneyTargets}
                onClick={() => setDialog({ kind: "bulk", action: "percent" })}
              />
              <Button
                label="Clear"
                size="sm"
                variant="ghost"
                isDisabled={selectedTargets.length === 0}
                onClick={() => apply(clearCells(selectedTargets))}
              />
            </>
          )}
        </div>
        <div className="pg-scroll">
          <table
            ref={gridRef}
            // A data grid is a table that takes keys: the ARIA grid pattern puts the role
            // on the table itself.
            // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-to-interactive-role
            role="grid"
            tabIndex={-1}
            aria-label={label}
            aria-rowcount={rows.length + 1}
            aria-colcount={gridColumns.length}
            aria-multiselectable="true"
            aria-readonly={isReadOnly || undefined}
            className="pg-table"
            onKeyDown={onKeyDown}
            onCopy={onCopy}
            onPaste={onPaste}
          >
            <thead className="pg-head">
              <tr aria-rowindex={1} className="pg-row">
                {gridColumns.map((column, col) => (
                  <th
                    key={column}
                    scope="col"
                    className="pg-cell"
                    aria-colindex={col + 1}
                    data-numeric={column === "name" ? undefined : ""}
                  >
                    {column === "name"
                      ? "Product"
                      : isEditableColumn(column) && column !== "breaks"
                        ? `${HEADERS[column]}, ${currency}`
                        : HEADERS[column]}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="pg-body">
              {rows.map((r, row) => (
                <tr
                  key={r.kind === "product" ? r.product.id : r.variant.variant}
                  className="pg-row"
                  aria-rowindex={row + 2}
                  data-kind={r.kind === "product" ? "product" : undefined}
                >
                  {gridColumns.map((column, col) => {
                    const cell = { row, col };
                    const target = targetAt(cell);
                    const key = target ? cellKey(target.variant, target.column) : undefined;
                    const error = key ? checked.errors.get(key) : undefined;
                    const isFocus = current.focus.row === row && current.focus.col === col;
                    const isEditing =
                      editing?.cell.row === row && editing.cell.col === col ? editing : null;
                    return (
                      <td
                        key={column}
                        className="pg-cell"
                        data-cell={`${row}:${col}`}
                        data-numeric={column === "name" ? undefined : ""}
                        data-dirty={key && checked.dirty.has(key) ? "" : undefined}
                        data-editable={target ? "" : undefined}
                        aria-colindex={col + 1}
                        aria-selected={contains(rect, cell)}
                        data-readonly={target ? undefined : ""}
                        aria-invalid={error ? true : undefined}
                        aria-expanded={
                          r.kind === "product" && column === "name" ? !r.isCollapsed : undefined
                        }
                        title={error}
                        tabIndex={isFocus ? 0 : -1}
                        onMouseDown={(e) => onCellMouseDown(e, cell)}
                        onMouseEnter={(e) => onCellMouseEnter(e, cell)}
                        onDoubleClick={() => activate(cell, "edit")}
                      >
                        {isEditing ? (
                          <CellInput
                            editing={isEditing}
                            label={`${column === "name" ? "Name" : HEADERS[column]}, ${rowTitle(r)}`}
                            onChange={(text) => setEditing({ ...isEditing, text })}
                            onCommit={(by) =>
                              commit(isEditing, clampCell({ row: row + by.row, col: col + by.col }))
                            }
                            onTab={(backwards) => commit(isEditing, tabFrom(cell, size, backwards))}
                            onCancel={() => {
                              setEditing(null);
                              focusCell(cell);
                            }}
                            onLeave={() => {
                              if (target) apply([{ ...target, text: isEditing.text }]);
                              setEditing(null);
                            }}
                          />
                        ) : (
                          renderCell(r, column)
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length === 0 && (
            <div className="pg-empty">
              <Text color="secondary">No products here.</Text>
            </div>
          )}
        </div>
        <output className="pg-status" aria-live="polite">
          {status && (
            <span className={status === notice ? undefined : "pg-status-error"}>{status}</span>
          )}
        </output>
      </div>
      {dialog?.kind === "breaks" && (
        <BreaksDialog
          title={dialog.title}
          currency={currency}
          digits={digits}
          fixedPrice={currentDraft(drafts, dialog.variant, digits).fixed_price}
          value={currentDraft(drafts, dialog.variant, digits).breaks}
          onSave={(text) => apply([{ variant: dialog.variant.variant, column: "breaks", text }])}
          onClose={() => {
            setDialog(null);
            refocusAfterDialog(current.focus);
          }}
        />
      )}
      {dialog?.kind === "bulk" && (
        <BulkDialog
          action={dialog.action}
          currency={currency}
          digits={digits}
          cells={selectedTargets.filter((t) => t.column !== "breaks").length}
          onApply={(value) => {
            if (dialog.action === "amount") apply(setTo(selectedTargets, value, digits));
            else {
              const { changes, skipped } = percentOfBase(
                selectedTargets,
                value,
                variants,
                currency,
                digits,
              );
              apply(changes, skipped);
            }
          }}
          onClose={() => {
            setDialog(null);
            refocusAfterDialog(current.focus);
          }}
        />
      )}
    </div>
  );

  function renderCell(r: PriceGridRow, column: GridColumn): ReactNode {
    if (column === "name") {
      if (r.kind === "product") {
        const Icon = r.isCollapsed ? ChevronRightIcon : ChevronDownIcon;
        return (
          <span className="pg-name">
            <button
              type="button"
              tabIndex={-1}
              className="pg-toggle"
              aria-label={
                r.isCollapsed
                  ? `Show ${r.product.title}'s variants`
                  : `Hide ${r.product.title}'s variants`
              }
              onClick={() => toggle(r.product.id)}
            >
              <Icon width={16} height={16} />
            </button>
            <span>{r.product.title}</span>
            <Text type="supporting" color="secondary">
              {r.product.variants.length === 1
                ? "1 variant"
                : `${r.product.variants.length} variants`}
            </Text>
          </span>
        );
      }
      const rowError = rowErrors[r.variant.variant];
      return (
        <span className={r.isSingle ? "pg-name" : "pg-name pg-variant"}>
          <span className="pg-name-text">
            <span>{r.isSingle ? r.product.title : r.variant.title}</span>
            {rowError ? (
              <span className="pg-row-error">{rowError}</span>
            ) : (
              r.variant.sku && (
                <Text type="supporting" color="secondary">
                  {r.variant.sku}
                </Text>
              )
            )}
          </span>
        </span>
      );
    }
    if (r.kind !== "variant") return null;
    const variant = r.variant;
    switch (column) {
      case "base_price":
        return moneyCell(variant.base_price, currencies);
      case "compare_at_price":
        return moneyCell(variant.compare_at_price, currencies);
      case "list_price": {
        const price = listPrice(variant);
        if (!price) return <span className="pg-none">—</span>;
        return (
          <span
            className={price.isAdjusted ? "pg-derived" : undefined}
            title={price.isAdjusted ? "From the list's adjustment" : "The fixed price"}
          >
            {formatMoney(price.money, currencies)}
          </span>
        );
      }
      case "breaks": {
        const text = currentDraft(drafts, variant, digits).breaks;
        if (text.trim() === "") return <span className="pg-none">—</span>;
        const parsed = parseBreaks(text, digits, currency);
        return parsed.ok ? breaksLabel(parsed.value.length) : text;
      }
      default: {
        const text = currentDraft(drafts, variant, digits)[column];
        if (text.trim() === "") return <span className="pg-none">—</span>;
        return normalizeCell(column, text, digits, currency);
      }
    }
  }
}

function rowTitle(r: PriceGridRow): string {
  return r.kind === "variant" && !r.isSingle
    ? `${r.product.title} / ${r.variant.title}`
    : r.product.title;
}

/** A read-only amount as copied: plain decimals, for a spreadsheet to take as numbers. */
function plainAmount(money: Money | null, currencies: Currencies): string {
  return money ? formatAmount(money.amount, currencies[money.currency] ?? 2) : "";
}

function moneyCell(money: Money | null, currencies: Currencies): ReactNode {
  return money ? formatMoney(money, currencies) : <span className="pg-none">—</span>;
}

/** The input a cell shows while it's edited. */
function CellInput({
  editing,
  label,
  onChange,
  onCommit,
  onTab,
  onCancel,
  onLeave,
}: {
  editing: Editing;
  label: string;
  onChange: (text: string) => void;
  /** Commit and move by this many rows and columns. */
  onCommit: (by: Cell) => void;
  onTab: (backwards: boolean) => void;
  onCancel: () => void;
  /** Focus went elsewhere (a click on another cell or outside): commit, stay put. */
  onLeave: () => void;
}) {
  // Set once a key has committed or cancelled, so the blur that follows as focus moves
  // doesn't commit a second time (or commit what Escape threw away).
  const isDone = useRef(false);
  const finish = (action: () => void) => {
    isDone.current = true;
    action();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const moves: Record<string, Cell> = {
      ArrowUp: { row: -1, col: 0 },
      ArrowDown: { row: 1, col: 0 },
      ArrowLeft: { row: 0, col: -1 },
      ArrowRight: { row: 0, col: 1 },
    };
    if (e.key === "Enter" && !e.nativeEvent.isComposing) {
      e.preventDefault();
      finish(() => onCommit({ row: e.shiftKey ? -1 : 1, col: 0 }));
    } else if (e.key === "Tab") {
      e.preventDefault();
      finish(() => onTab(e.shiftKey));
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      finish(onCancel);
    } else if (editing.mode === "replace" && moves[e.key]) {
      // Typed over: the arrows move on, as in a spreadsheet. Opened with Enter or F2
      // they move the caret.
      e.preventDefault();
      const move = moves[e.key];
      finish(() => onCommit(move));
    }
  };

  return (
    <input
      ref={focusOnMount}
      className="pg-input"
      aria-label={label}
      value={editing.text}
      inputMode="decimal"
      autoComplete="off"
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={onKeyDown}
      onBlur={() => {
        if (!isDone.current) onLeave();
      }}
    />
  );
}

/** Focuses the input as it appears, the caret after its text. */
function focusOnMount(input: HTMLInputElement | null) {
  if (!input || input === document.activeElement) return;
  input.focus();
  input.setSelectionRange(input.value.length, input.value.length);
}
