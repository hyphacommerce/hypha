"use client";

/**
 * A price list's prices as a full-window spreadsheet: products with their variants
 * beneath, a column per price, edited in place and saved together. It fills its
 * container (the page gives it the viewport): two bars above, the grid scrolling inside
 * with its header row and names column held in place, a status line below.
 *
 * Usage:
 *
 *     <PriceGrid
 *       pageKey={`${q}|${category}|${cursor}`}  // a new page starts the grid afresh
 *       currency={list.currency}
 *       products={rows}                 // PriceGridProduct[]: a page of the catalog
 *       columns={["base_price", "list_price", "fixed_price", "breaks"]}
 *       renderChrome={({ isDirty, confirmLeave }) => ({
 *         title: <BackAndName />,       // the first bar's start
 *         filters: <Filters disabled={isDirty} />,  // the second bar's start
 *         tools: <ExportImport />,      // the second bar's end
 *         pager: <Pages disabled={isDirty} />,      // the status line's end
 *         banner: error && <Banner … />,  // above the grid
 *       })}
 *       empty={<EmptyState … />}        // instead of the grid when there are no rows
 *       onSave={(diff) => saveFixedPrices(list.id, diff)}
 *     />
 *
 * - `currency` is the list's; editable cells are in its minor units (`useCurrencies`),
 *   and the column headers name it, so cells show plain amounts.
 * - `products` (`lib/price-grid/types.ts`): each variant's base and compare-at prices,
 *   `adjusted_price` (what the list's adjustment makes of the base, null when the
 *   adjustment doesn't cover it) and `fixed` (`{price, compare_at, breaks}` or null).
 *   A product whose only variant has no title (no options) is one row.
 * - `columns` picks and orders the price columns (all six by default): `base_price`,
 *   `compare_at_price`, `list_price` (the price the list gives, greyed when it comes
 *   from the adjustment) are read-only; `fixed_price`, `fixed_compare_at` and `breaks`
 *   are edited. The product and SKU columns always come first.
 * - `renderChrome` fills the bars around the grid. It gets `isDirty` (unsaved changes) and
 *   `confirmLeave(leave)`, which runs `leave` at once without changes and otherwise
 *   after the user agrees to drop them.
 * - `onSave(diff)` gets a `PriceGridDiff`: `upserts` (`{variant, price, compare_at,
 *   breaks}`, money as `{amount, currency}`, for `PUT …/fixed-prices` in chunks of
 *   1,000) and `clears` (`{variant}`, for `DELETE …/fixed-prices`). It answers
 *   `{error?, rowErrors?}`: nothing means saved and the drafts go (refresh `products`
 *   in the action); `rowErrors` (by variant ID) keeps those variants' drafts with the
 *   message on their row and drops the rest as saved; `error` alone keeps every draft.
 * - `isReadOnly` (no `pricing:write`) shows the prices without editing.
 *
 * Mouse: a click selects a cell, a click on the selected cell (or a double click) edits
 * it, a click on a breaks cell opens its editor, dragging or Shift+click selects a
 * rectangle. Keys (`lib/price-grid/keys.ts`): typing replaces the cell, Enter or F2 edits
 * in place (a breaks cell opens its editor; a product's name folds it), Enter commits
 * and moves down, Tab/Shift+Tab commit and move to the next editable cell, Escape
 * cancels, arrows move (Shift extends, Ctrl/Cmd to the edge), Delete clears, Ctrl/Cmd+C
 * copies the selection as TSV, Ctrl/Cmd+V pastes a spreadsheet's cells, Ctrl/Cmd+D
 * fills down, Ctrl/Cmd+A selects everything, Ctrl/Cmd+S saves.
 */

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Kbd } from "@astryxdesign/core/Kbd";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  ExclamationCircleIcon,
  PencilIcon,
} from "@heroicons/react/16/solid";
import {
  type ClipboardEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useEffect,
  useEffectEvent,
  useMemo,
  useRef,
  useState,
  useTransition,
} from "react";

import { useCurrencies } from "@/components/currencies";
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
import { gridCommand, isSaveKey } from "@/lib/price-grid/keys";
import { cellAmount, formatAmount, parseAmount } from "@/lib/price-grid/money";
import { type PriceGridRow, variantsById, visibleRows } from "@/lib/price-grid/rows";
import {
  type Cell,
  type Selection,
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

/** What `renderChrome` is told. */
export type PriceGridState = {
  isDirty: boolean;
  /** Runs `leave` now without unsaved changes; with some, once the user drops them. */
  confirmLeave: (leave: () => void) => void;
};

/** What `renderChrome` puts around the grid. */
export type PriceGridChrome = {
  title: ReactNode;
  filters?: ReactNode;
  tools?: ReactNode;
  pager?: ReactNode;
  /** A message above the grid (a refused filter, a file that won't import). */
  banner?: ReactNode;
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
  base_price: "Base",
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
  | { kind: "leave"; leave: () => void }
  | null;

/** Column 0 is the names, 1 the SKUs; the rest are `columns`. */
type GridColumn = "name" | "sku" | PriceGridColumn;

const START: Selection = selectCell({ row: 0, col: 0 });

export function PriceGrid({
  pageKey,
  currency,
  products,
  columns = ALL_COLUMNS,
  isReadOnly = false,
  renderChrome,
  empty,
  isLoading = false,
  label = "Prices",
  onSave,
}: {
  /** Names the page shown: a new one drops the drafts, the selection and the folds. */
  pageKey: string;
  currency: string;
  products: readonly PriceGridProduct[];
  columns?: readonly PriceGridColumn[];
  isReadOnly?: boolean;
  renderChrome: (state: PriceGridState) => PriceGridChrome;
  /** Shown in the grid's place when there are no rows. */
  empty?: ReactNode;
  /** Another page is on its way: the grid dims meanwhile. */
  isLoading?: boolean;
  /** The grid's accessible name. */
  label?: string;
  onSave: (diff: PriceGridDiff) => Promise<PriceGridSaveResult>;
}) {
  const currencies = useCurrencies();
  const digits = currencies[currency] ?? 2;

  const [drafts, setDrafts] = useState<Drafts>(() => new Map());
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [selection, setSelection] = useState<Selection>(START);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [dialog, setDialog] = useState<Dialogs>(null);
  const [notice, setNotice] = useState<string>();
  const [saveError, setSaveError] = useState<string>();
  const [rowErrors, setRowErrors] = useState<Readonly<Record<string, string>>>({});
  const [isPending, startTransition] = useTransition();
  const gridRef = useRef<HTMLTableElement>(null);
  // The cell a press began on, and whether it was the one selected already: released on
  // the same cell, that press edits it rather than selecting it again.
  const press = useRef<{ cell: Cell; wasSelected: boolean } | null>(null);

  // A new page (filters or cursor) starts afresh, as if the grid were mounted again,
  // while the bars around it (the search box's text and focus) stay.
  const [shownPage, setShownPage] = useState(pageKey);
  if (pageKey !== shownPage) {
    setShownPage(pageKey);
    setDrafts(new Map());
    setCollapsed(new Set());
    setSelection(START);
    setEditing(null);
    setRowErrors({});
    setSaveError(undefined);
    setNotice(undefined);
  }

  const rows = useMemo(() => visibleRows(products, collapsed), [products, collapsed]);
  const variants = useMemo(() => variantsById(products), [products]);
  const checked = useMemo(
    () => review(variants, drafts, currency, digits),
    [variants, drafts, currency, digits],
  );
  const changeCount = checked.changedVariants;
  const isDirty = changeCount > 0;

  const gridColumns: GridColumn[] = ["name", "sku", ...columns];
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
  const isMulti = !sameCell(current.anchor, current.focus);

  const targetAt = ({ row, col }: Cell): Target | null => {
    const r = rows[row];
    const column = gridColumns[col];
    if (isReadOnly || r?.kind !== "variant" || !column || column === "name" || column === "sku") {
      return null;
    }
    return isEditableColumn(column) ? { variant: r.variant.variant, column } : null;
  };
  const isEditable = (cell: Cell) => targetAt(cell) !== null;
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

  /** Enter, F2, a click on the selected cell: edit it, open its breaks, or fold its product. */
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

  /** The open cell's text as a change, to commit. */
  const editingChange = (edit: Editing): CellChange[] => {
    const target = targetAt(edit.cell);
    return target ? [{ ...target, text: edit.text }] : [];
  };

  /** A key ended the edit: keep the text and move on. */
  const commit = (edit: Editing, next: Cell) => {
    apply(editingChange(edit));
    setEditing(null);
    select(selectCell(next));
  };

  const save = (pending: readonly CellChange[] = []) => {
    if (isReadOnly || isPending) return;
    // Typing goes on while the save runs. Drafts are replaced, never mutated, so a draft
    // that's still the one sent is what was saved; any other was typed meanwhile.
    const sent = applyChanges(drafts, variants, pending, digits, currency);
    if (sent !== drafts) setDrafts(sent);
    const result = review(variants, sent, currency, digits);
    if (result.changedVariants === 0) return;
    if (result.errors.size > 0) {
      setNotice("Fix the cells marked in red, then save.");
      return;
    }
    startTransition(async () => {
      const answer = await onSave(result.diff);
      const failed = answer.rowErrors ?? {};
      const failedIds = Object.keys(failed);
      setRowErrors(failed);
      if (answer.error && failedIds.length === 0) {
        setSaveError(answer.error);
        return;
      }
      setDrafts(
        (before) =>
          new Map([...before].filter(([id, d]) => d !== sent.get(id) || failedIds.includes(id))),
      );
      setSaveError(
        failedIds.length > 0
          ? (answer.error ??
              `${failedIds.length} ${failedIds.length === 1 ? "variant wasn't" : "variants weren't"} saved: see the rows marked.`)
          : undefined,
      );
      setNotice(failedIds.length > 0 ? undefined : "Saved.");
    });
  };

  const discard = () => {
    setDrafts(new Map());
    setRowErrors({});
    setSaveError(undefined);
    setEditing(null);
    setNotice(undefined);
  };

  const confirmLeave = (leave: () => void) => {
    if (isDirty) setDialog({ kind: "leave", leave });
    else leave();
  };

  // Ctrl/Cmd+S saves from anywhere on the page, an open cell's text included.
  const onWindowKey = useEffectEvent((e: globalThis.KeyboardEvent) => {
    if (!isSaveKey({ key: e.key, shift: e.shiftKey, mod: e.metaKey || e.ctrlKey, alt: e.altKey })) {
      return;
    }
    // Never the browser's "Save page as", which would only save the HTML.
    e.preventDefault();
    if (dialog) return;
    const pending = editing ? editingChange(editing) : [];
    if (editing) {
      setEditing(null);
      focusCell(editing.cell);
    }
    save(pending);
  });
  useEffect(() => {
    const listener = (e: globalThis.KeyboardEvent) => onWindowKey(e);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  // Closing the tab or reloading with unsaved changes asks first.
  useEffect(() => {
    if (!isDirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [isDirty]);

  const listPrice = (variant: PriceGridVariant): { money: Money; isAdjusted: boolean } | null => {
    const text = currentDraft(drafts, variant, digits).fixed_price;
    if (text.trim() === "") {
      return variant.adjusted_price ? { money: variant.adjusted_price, isAdjusted: true } : null;
    }
    const amount = parseAmount(text, digits, currency);
    return amount.ok ? { money: { amount: amount.value, currency }, isAdjusted: false } : null;
  };

  const plain = (money: Money | null) =>
    money ? formatAmount(money.amount, currencies[money.currency] ?? 2) : "";

  const copyText = (cell: Cell): string => {
    const r = rows[cell.row];
    const column = gridColumns[cell.col];
    if (!r || !column) return "";
    if (column === "name") return r.kind === "variant" ? rowTitle(r) : r.product.title;
    if (r.kind !== "variant") return "";
    if (column === "sku") return r.variant.sku ?? "";
    if (isEditableColumn(column)) return textAt({ variant: r.variant.variant, column });
    if (column === "base_price") return plain(r.variant.base_price);
    if (column === "compare_at_price") return plain(r.variant.compare_at_price);
    return plain(listPrice(r.variant)?.money ?? null);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTableElement>) => {
    if (editing || !(e.target instanceof Node) || !e.currentTarget.contains(e.target)) return;
    const command = gridCommand({
      key: e.key,
      shift: e.shiftKey,
      mod: e.metaKey || e.ctrlKey,
      alt: e.altKey,
    });
    if (!command) return;
    const at = current.focus;
    switch (command.kind) {
      case "move":
        e.preventDefault();
        return select(moveBy(current, command.rows, command.cols, size, command.extend));
      case "tab": {
        const next = isReadOnly
          ? tabFrom(at, size, command.backwards)
          : tabToEditable(at, size, command.backwards, isEditable);
        // At either end, Tab leaves the grid as it would any other control.
        if (sameCell(next, at)) return;
        e.preventDefault();
        return select(selectCell(next));
      }
      case "edit":
        e.preventDefault();
        return activate(at, "edit");
      case "space":
        if (rows[at.row]?.kind === "product" && at.col === 0) {
          e.preventDefault();
          return activate(at, "edit");
        }
        return typeInto(e, " ");
      case "type":
        return typeInto(e, command.text);
      case "collapse":
        return select(selectCell(at));
      case "clear":
        e.preventDefault();
        return apply(clearCells(selectedTargets));
      case "fillDown":
        e.preventDefault();
        return apply(fillDown(rect, targetAt, textAt));
      case "selectAll":
        e.preventDefault();
        return select(selectAll(size));
      case "save":
        // The window's listener saves; this only keeps the key from going further.
        return;
    }
  };

  /** Typing on a money cell starts over with what's typed, as in a spreadsheet. */
  const typeInto = (e: KeyboardEvent, text: string) => {
    const target = targetAt(current.focus);
    if (!target || target.column === "breaks") return;
    e.preventDefault();
    activate(current.focus, "replace", text);
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
    press.current = null;
    // A click into the cell being edited is the input's; leaving it commits on blur.
    if (e.button !== 0 || (editing && sameCell(editing.cell, cell))) return;
    if (e.shiftKey) {
      // Keep the browser from selecting the page's text between the two cells.
      e.preventDefault();
      setSelection(extendTo(current, cell));
      focusCell(cell);
    } else {
      press.current = { cell, wasSelected: !isMulti && sameCell(current.focus, cell) };
      setSelection(selectCell(cell));
    }
    setNotice(undefined);
  };

  /** The end of a click (keys have their own way to each of these: Enter). */
  const onCellMouseUp = (e: MouseEvent, cell: Cell) => {
    const began = press.current;
    press.current = null;
    if (e.button !== 0 || !began || !sameCell(began.cell, cell)) return;
    const target = targetAt(cell);
    // A breaks cell opens its editor at once: there's nothing to type into.
    if (target?.column === "breaks" || (target && began.wasSelected)) activate(cell, "edit");
  };

  const onCellMouseEnter = (e: MouseEvent, cell: Cell) => {
    // Dragging with the button held draws the rectangle.
    if (e.buttons & 1 && !editing) setSelection(extendTo(current, cell));
  };

  const focusedTarget = targetAt(current.focus);
  const focusedRow = rows[current.focus.row];
  const cellError =
    (focusedTarget && checked.errors.get(cellKey(focusedTarget.variant, focusedTarget.column))) ??
    (focusedRow?.kind === "variant" ? rowErrors[focusedRow.variant.variant] : undefined);
  const hasMoneyTargets = selectedTargets.some((t) => t.column !== "breaks");
  const parts = renderChrome({ isDirty, confirmLeave });

  return (
    <div className="pe">
      <header className="pe-bar">
        <div className="pe-title">{parts.title}</div>
        <div className="pe-save">
          {!isReadOnly && (
            <>
              <span className="pe-count" data-state={checked.errors.size > 0 ? "error" : undefined}>
                {changeCount === 0
                  ? "No unsaved changes"
                  : `${changeCount} ${changeCount === 1 ? "variant" : "variants"} changed${
                      checked.errors.size > 0 ? `, ${checked.errors.size} to fix` : ""
                    }`}
              </span>
              <Button
                label="Discard"
                size="sm"
                variant="ghost"
                isDisabled={!isDirty || isPending}
                onClick={discard}
              />
              <Button
                label="Save"
                size="sm"
                variant="primary"
                tooltip="Save all changes (Ctrl/⌘ S)"
                isLoading={isPending}
                isDisabled={!isDirty || checked.errors.size > 0}
                onClick={() => save()}
              />
            </>
          )}
        </div>
      </header>
      <div className="pe-tools">
        {parts.filters}
        {!isReadOnly && rows.length > 0 && (
          <div className="pe-group">
            <Button
              label="Set to…"
              size="sm"
              variant="ghost"
              isDisabled={!hasMoneyTargets}
              onClick={() => setDialog({ kind: "bulk", action: "amount" })}
            />
            <Button
              label="% of base…"
              size="sm"
              variant="ghost"
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
          </div>
        )}
        <div className="pe-spacer" />
        {parts.tools}
      </div>
      {(saveError || parts.banner) && (
        <div className="pe-banners">
          {parts.banner}
          {saveError && <Banner status="error" title={saveError} />}
        </div>
      )}
      <div
        className="pg-scroll pending-content"
        aria-busy={isLoading}
        data-pending={isLoading || undefined}
      >
        {rows.length === 0 ? (
          <div className="pg-empty">{empty}</div>
        ) : (
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
            data-multi={isMulti || undefined}
            onKeyDown={onKeyDown}
            onCopy={onCopy}
            onPaste={onPaste}
          >
            <colgroup>
              {gridColumns.map((column) => (
                <col key={column} data-col={column} />
              ))}
            </colgroup>
            <thead className="pg-head">
              <tr aria-rowindex={1} className="pg-row">
                {gridColumns.map((column, col) => (
                  <th
                    key={column}
                    scope="col"
                    className="pg-cell"
                    aria-colindex={col + 1}
                    data-col={column}
                    data-numeric={column === "name" || column === "sku" ? undefined : ""}
                    data-editable={
                      !isReadOnly &&
                      column !== "name" &&
                      column !== "sku" &&
                      isEditableColumn(column)
                        ? ""
                        : undefined
                    }
                  >
                    {headerOf(column)}
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
                  data-row-error={
                    r.kind === "variant" && rowErrors[r.variant.variant] ? "" : undefined
                  }
                >
                  {gridColumns.map((column, col) => {
                    const cell = { row, col };
                    const target = targetAt(cell);
                    const key = target ? cellKey(target.variant, target.column) : undefined;
                    const error = key ? checked.errors.get(key) : undefined;
                    const isFocus = sameCell(current.focus, cell);
                    const isEditing = editing && sameCell(editing.cell, cell) ? editing : null;
                    return (
                      <td
                        key={column}
                        className="pg-cell"
                        data-col={column}
                        data-cell={`${row}:${col}`}
                        data-numeric={column === "name" || column === "sku" ? undefined : ""}
                        data-dirty={key && checked.dirty.has(key) ? "" : undefined}
                        data-editable={target ? "" : undefined}
                        data-editing={isEditing ? "" : undefined}
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
                        onMouseUp={(e) => onCellMouseUp(e, cell)}
                        onMouseEnter={(e) => onCellMouseEnter(e, cell)}
                        onDoubleClick={() => {
                          if (target?.column !== "breaks") activate(cell, "edit");
                        }}
                      >
                        {isEditing ? (
                          <CellInput
                            editing={isEditing}
                            label={`${column === "name" || column === "sku" ? "Name" : HEADERS[column]}, ${rowTitle(r)}`}
                            onChange={(text) => setEditing({ ...isEditing, text })}
                            onCommit={(by) =>
                              commit(
                                isEditing,
                                by.row === 0
                                  ? clampCell({ row, col: col + by.col })
                                  : nextEditableInColumn(
                                      cell,
                                      by.row > 0 ? 1 : -1,
                                      size,
                                      isEditable,
                                    ),
                              )
                            }
                            onTab={(backwards) =>
                              commit(isEditing, tabToEditable(cell, size, backwards, isEditable))
                            }
                            onCancel={() => {
                              setEditing(null);
                              focusCell(cell);
                            }}
                            onLeave={() => {
                              apply(editingChange(isEditing));
                              setEditing(null);
                            }}
                          />
                        ) : (
                          renderCell(r, column, error)
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <footer className="pe-foot">
        <output className="pe-status" aria-live="polite">
          {cellError ? (
            <span className="pe-status-error">
              <ExclamationCircleIcon width={14} height={14} aria-hidden />
              {cellError}
            </span>
          ) : notice ? (
            <span>{notice}</span>
          ) : isReadOnly ? (
            <span>Your role can see these prices but not change them.</span>
          ) : (
            <span className="pe-hint">
              Click a cell and type. <Kbd keys="enter" /> to edit, <Kbd keys="tab" /> or arrows to
              move, <Kbd keys="mod+s" /> to save.
            </span>
          )}
        </output>
        {parts.pager}
      </footer>
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
      {dialog?.kind === "leave" && (
        <AlertDialog
          isOpen
          onOpenChange={(open) => !open && setDialog(null)}
          title="Discard unsaved changes?"
          description={`${changeCount} ${changeCount === 1 ? "variant has" : "variants have"} changes that aren't saved. Leaving drops them.`}
          cancelLabel="Keep editing"
          actionLabel="Discard changes"
          onAction={() => {
            const { leave } = dialog;
            discard();
            setDialog(null);
            leave();
          }}
        />
      )}
    </div>
  );

  function headerOf(column: GridColumn): ReactNode {
    if (column === "name") return "Product";
    if (column === "sku") return "SKU";
    const name = HEADERS[column];
    const editable = !isReadOnly && isEditableColumn(column);
    const showsCurrency = column !== "breaks" && column !== "base_price";
    return (
      <span className="pg-head-label">
        {editable && <PencilIcon width={12} height={12} aria-hidden className="pg-head-icon" />}
        <span>{name}</span>
        {showsCurrency && <span className="pg-head-currency">{currency}</span>}
      </span>
    );
  }

  function renderCell(r: PriceGridRow, column: GridColumn, error: string | undefined): ReactNode {
    if (column === "name") {
      if (r.kind === "product") {
        const Icon = r.isCollapsed ? ChevronRightIcon : ChevronDownIcon;
        const count = r.product.variants.length;
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
              <Icon width={14} height={14} />
            </button>
            <span className="pg-name-title">{r.product.title}</span>
            <span className="pg-muted">{r.product.handle}</span>
            <span className="pg-muted">{count === 1 ? "1 variant" : `${count} variants`}</span>
          </span>
        );
      }
      const rowError = rowErrors[r.variant.variant];
      return (
        <span className={r.isSingle ? "pg-name" : "pg-name pg-variant"}>
          <span className="pg-name-title" title={rowTitle(r)}>
            {r.isSingle ? r.product.title : r.variant.title}
          </span>
          {rowError && (
            <span className="pg-row-error" title={rowError}>
              <ExclamationCircleIcon width={14} height={14} aria-label={rowError} />
            </span>
          )}
        </span>
      );
    }
    if (r.kind !== "variant") return null;
    const variant = r.variant;
    switch (column) {
      case "sku":
        return variant.sku ?? <span className="pg-none">—</span>;
      case "base_price":
        return readOnlyAmount(variant.base_price);
      case "compare_at_price":
        return readOnlyAmount(variant.compare_at_price);
      case "list_price": {
        const price = listPrice(variant);
        if (!price) return <span className="pg-none">—</span>;
        return (
          <span
            className={price.isAdjusted ? "pg-derived" : undefined}
            title={price.isAdjusted ? "From the list's adjustment" : "The fixed price"}
          >
            {readOnlyAmount(price.money)}
          </span>
        );
      }
      case "breaks": {
        const text = currentDraft(drafts, variant, digits).breaks;
        if (text.trim() === "") {
          return isReadOnly ? <span className="pg-none">—</span> : <EmptyEditable label="Add" />;
        }
        const parsed = parseBreaks(text, digits, currency);
        return (
          <EditableValue error={error}>
            {parsed.ok ? breaksLabel(parsed.value.length) : text}
          </EditableValue>
        );
      }
      default: {
        const text = currentDraft(drafts, variant, digits)[column];
        if (text.trim() === "") {
          return isReadOnly ? <span className="pg-none">—</span> : <EmptyEditable />;
        }
        return (
          <EditableValue error={error}>
            {normalizeCell(column, text, digits, currency)}
          </EditableValue>
        );
      }
    }
  }

  function readOnlyAmount(money: Money | null): ReactNode {
    if (!money) return <span className="pg-none">—</span>;
    return cellAmount(money, currencies[money.currency] ?? 2, currency);
  }
}

/** An editable cell's text, with a mark when it can't be saved as it is. */
function EditableValue({ error, children }: { error?: string; children: ReactNode }) {
  return (
    <span className="pg-value">
      {error && <ExclamationCircleIcon width={12} height={12} aria-hidden className="pg-flag" />}
      {children}
    </span>
  );
}

/** An empty editable cell: blank at rest, a pencil (and `label`) under the pointer. */
function EmptyEditable({ label }: { label?: string }) {
  return (
    <span className="pg-empty-cell" aria-hidden>
      <PencilIcon width={12} height={12} />
      {label}
    </span>
  );
}

function rowTitle(r: PriceGridRow): string {
  return r.kind === "variant" && !r.isSingle
    ? `${r.product.title} / ${r.variant.title}`
    : r.product.title;
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
      // Typed over: the arrows move on, as in a spreadsheet. Opened with Enter, F2 or a
      // click they move the caret.
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
