// The grid's unsaved changes. Each touched variant has a draft: the text of its three
// editable cells, kept as typed so a wrong entry stays visible with its error. `review`
// turns the drafts into the save's diff, the cells' errors and which cells changed;
// every way of editing (typing, paste, fill, bulk actions) is a list of `CellChange`s
// applied by `applyChanges`.

import { type BreakAmount, breakProblems, formatBreaks, parseBreaks } from "./breaks.ts";
import { adjustAmount, formatAmount, parseAmount } from "./money.ts";
import type { Cell, Rect } from "./selection.ts";
import type { Placed } from "./tsv.ts";
import type {
  EditableColumn,
  FixedPrice,
  FixedPriceUpsert,
  Money,
  PriceGridDiff,
  PriceGridVariant,
} from "./types.ts";
import { EDITABLE_COLUMNS } from "./types.ts";

export type Draft = Readonly<Record<EditableColumn, string>>;

/** By variant ID; only variants someone touched. */
export type Drafts = ReadonlyMap<string, Draft>;

export type CellChange = { variant: string; column: EditableColumn; text: string };

/** A cell's key in `Review`'s maps. */
export function cellKey(variant: string, column: EditableColumn): string {
  return `${variant}:${column}`;
}

const EMPTY: Draft = { fixed_price: "", fixed_compare_at: "", breaks: "" };

/** The cells' text for a variant's stored fixed price. */
export function originalDraft(variant: PriceGridVariant, digits: number): Draft {
  const { fixed } = variant;
  if (!fixed) return EMPTY;
  return {
    fixed_price: formatAmount(fixed.price.amount, digits),
    fixed_compare_at: fixed.compare_at ? formatAmount(fixed.compare_at.amount, digits) : "",
    breaks: formatBreaks(
      fixed.breaks.map((b) => ({ min_quantity: b.min_quantity, amount: b.price.amount })),
      digits,
    ),
  };
}

/** The variant's cells as they stand: its draft, else what's stored. */
export function currentDraft(drafts: Drafts, variant: PriceGridVariant, digits: number): Draft {
  return drafts.get(variant.variant) ?? originalDraft(variant, digits);
}

/**
 * A cell's text in its canonical form ("12.5" → "12.50", "10:8" → "10=8.00"), or as
 * typed when it doesn't parse. Two texts that normalize alike mean the same price.
 */
export function normalizeCell(
  column: EditableColumn,
  text: string,
  digits: number,
  currency: string,
): string {
  const trimmed = text.trim();
  if (trimmed === "") return "";
  if (column === "breaks") {
    const breaks = parseBreaks(trimmed, digits, currency);
    return breaks.ok ? formatBreaks(breaks.value, digits) : trimmed;
  }
  const amount = parseAmount(trimmed, digits, currency);
  return amount.ok ? formatAmount(amount.value, digits) : trimmed;
}

function sameDraft(a: Draft, b: Draft, digits: number, currency: string): boolean {
  return (["fixed_price", "fixed_compare_at", "breaks"] as const).every(
    (column) =>
      normalizeCell(column, a[column], digits, currency) ===
      normalizeCell(column, b[column], digits, currency),
  );
}

/**
 * The drafts with `changes` applied. A variant whose cells come back to what's stored
 * loses its draft, so undoing an edit by hand leaves nothing to save. Changes to unknown
 * variants are ignored.
 */
export function applyChanges(
  drafts: Drafts,
  variants: ReadonlyMap<string, PriceGridVariant>,
  changes: readonly CellChange[],
  digits: number,
  currency: string,
): Drafts {
  if (changes.length === 0) return drafts;
  const next = new Map(drafts);
  for (const change of changes) {
    const variant = variants.get(change.variant);
    if (!variant) continue;
    const draft = { ...currentDraft(next, variant, digits), [change.column]: change.text };
    if (sameDraft(draft, originalDraft(variant, digits), digits, currency)) {
      next.delete(change.variant);
    } else {
      next.set(change.variant, draft);
    }
  }
  return next;
}

/** What a draft comes to: a fixed price to write, none at all, or problems by cell. */
export type Checked =
  | { kind: "price"; fixed: FixedPrice }
  | { kind: "none" }
  | { kind: "invalid"; errors: Partial<Record<EditableColumn, string>> };

/** A draft checked against the API's rules for a fixed price. */
export function checkDraft(draft: Draft, currency: string, digits: number): Checked {
  const errors: Partial<Record<EditableColumn, string>> = {};
  const priceText = draft.fixed_price.trim();
  const compareText = draft.fixed_compare_at.trim();
  const breaksText = draft.breaks.trim();
  if (priceText === "" && compareText === "" && breaksText === "") return { kind: "none" };

  let price: number | null = null;
  if (priceText === "") {
    errors.fixed_price = "Needs a fixed price, or clear the row.";
  } else {
    const parsed = parseAmount(priceText, digits, currency);
    if (parsed.ok) price = parsed.value;
    else errors.fixed_price = parsed.error;
  }

  let compareAt: number | null = null;
  if (compareText !== "") {
    const parsed = parseAmount(compareText, digits, currency);
    if (!parsed.ok) errors.fixed_compare_at = parsed.error;
    else if (price !== null && parsed.value <= price) {
      errors.fixed_compare_at = "Must be above the fixed price.";
    } else compareAt = parsed.value;
  }

  let breaks: BreakAmount[] = [];
  if (breaksText !== "") {
    const parsed = parseBreaks(breaksText, digits, currency);
    if (!parsed.ok) errors.breaks = parsed.error;
    else {
      const [problem] = breakProblems(parsed.value, price);
      // Without a parsable price the price cell already says so.
      if (problem && price !== null) errors.breaks = problem;
      breaks = parsed.value;
    }
  }

  if (Object.keys(errors).length > 0 || price === null) return { kind: "invalid", errors };
  const money = (amount: number): Money => ({ amount, currency });
  return {
    kind: "price",
    fixed: {
      price: money(price),
      compare_at: compareAt === null ? null : money(compareAt),
      breaks: breaks.map((b) => ({ min_quantity: b.min_quantity, price: money(b.amount) })),
    },
  };
}

function sameMoney(a: Money | null, b: Money | null): boolean {
  return (
    a === b || (a !== null && b !== null && a.amount === b.amount && a.currency === b.currency)
  );
}

/** Whether two fixed prices are the same price, compare-at and breaks. */
export function sameFixed(a: FixedPrice, b: FixedPrice): boolean {
  return (
    sameMoney(a.price, b.price) &&
    sameMoney(a.compare_at, b.compare_at) &&
    a.breaks.length === b.breaks.length &&
    a.breaks.every(
      (x, i) =>
        x.min_quantity === b.breaks[i]?.min_quantity &&
        sameMoney(x.price, b.breaks[i]?.price ?? null),
    )
  );
}

export type Review = {
  diff: PriceGridDiff;
  /** By `cellKey`: why the cell can't be saved. */
  errors: ReadonlyMap<string, string>;
  /** By `cellKey`: cells that differ from what's stored. */
  dirty: ReadonlySet<string>;
  /** Variants that change on save, or would once their errors are fixed. */
  changedVariants: number;
};

/** The drafts as the save sees them. */
export function review(
  variants: ReadonlyMap<string, PriceGridVariant>,
  drafts: Drafts,
  currency: string,
  digits: number,
): Review {
  const upserts: FixedPriceUpsert[] = [];
  const clears: { variant: string }[] = [];
  const errors = new Map<string, string>();
  const dirty = new Set<string>();
  let changedVariants = 0;

  for (const [id, draft] of drafts) {
    const variant = variants.get(id);
    if (!variant) continue;
    const original = originalDraft(variant, digits);
    for (const column of ["fixed_price", "fixed_compare_at", "breaks"] as const) {
      if (normalizeCell(column, draft[column], digits, currency) !== original[column]) {
        dirty.add(cellKey(id, column));
      }
    }
    const checked = checkDraft(draft, currency, digits);
    if (checked.kind === "invalid") {
      changedVariants++;
      for (const column of EDITABLE_COLUMNS) {
        const message = checked.errors[column];
        if (message) errors.set(cellKey(id, column), message);
      }
    } else if (checked.kind === "none") {
      if (variant.fixed) {
        changedVariants++;
        clears.push({ variant: id });
      }
    } else if (!variant.fixed || !sameFixed(checked.fixed, variant.fixed)) {
      changedVariants++;
      upserts.push({ variant: id, ...checked.fixed });
    }
  }
  return { diff: { upserts, clears }, errors, dirty, changedVariants };
}

/** A grid cell that can be edited, as the variant and column it stands for. */
export type Target = { variant: string; column: EditableColumn };

/** Pasted or filled cells as changes, skipping those that can't be edited. */
export function changesAt(
  placed: readonly Placed[],
  targetAt: (cell: Cell) => Target | null,
): { changes: CellChange[]; skipped: number } {
  const changes: CellChange[] = [];
  let skipped = 0;
  for (const p of placed) {
    const target = targetAt(p);
    if (target) changes.push({ ...target, text: p.text });
    else skipped++;
  }
  return { changes, skipped };
}

/**
 * Ctrl/Cmd+D: each column of the selection takes its first editable cell's text all the
 * way down. A selection one row high takes the nearest editable cell above it instead,
 * as spreadsheets do.
 */
export function fillDown(
  rect: Rect,
  targetAt: (cell: Cell) => Target | null,
  textAt: (target: Target) => string,
): CellChange[] {
  const changes: CellChange[] = [];
  for (let col = rect.left; col <= rect.right; col++) {
    let sourceRow = -1;
    if (rect.top === rect.bottom) {
      for (let row = rect.top - 1; row >= 0; row--) {
        if (targetAt({ row, col })) {
          sourceRow = row;
          break;
        }
      }
    } else {
      for (let row = rect.top; row <= rect.bottom; row++) {
        if (targetAt({ row, col })) {
          sourceRow = row;
          break;
        }
      }
    }
    const source = sourceRow >= 0 ? targetAt({ row: sourceRow, col }) : null;
    if (!source) continue;
    const text = textAt(source);
    for (let row = Math.max(rect.top, sourceRow + 1); row <= rect.bottom; row++) {
      const target = targetAt({ row, col });
      if (target) changes.push({ ...target, text });
    }
  }
  return changes;
}

/** "Set to an amount": every selected money cell takes `amount` (minor units). */
export function setTo(targets: readonly Target[], amount: number, digits: number): CellChange[] {
  const text = formatAmount(amount, digits);
  return targets.filter((t) => t.column !== "breaks").map((t) => ({ ...t, text }));
}

/**
 * "±x% of base": every selected money cell takes its variant's base price adjusted by
 * `bps`, rounded as the API rounds. A variant whose base price is in another currency
 * than the list's has no such price and is skipped.
 */
export function percentOfBase(
  targets: readonly Target[],
  bps: number,
  variants: ReadonlyMap<string, PriceGridVariant>,
  currency: string,
  digits: number,
): { changes: CellChange[]; skipped: number } {
  const changes: CellChange[] = [];
  let skipped = 0;
  for (const target of targets) {
    if (target.column === "breaks") continue;
    const base = variants.get(target.variant)?.base_price;
    const amount = base && base.currency === currency ? adjustAmount(base.amount, bps) : null;
    if (amount === null || amount < 0) skipped++;
    else changes.push({ ...target, text: formatAmount(amount, digits) });
  }
  return { changes, skipped };
}

/** Delete, Backspace and "Clear": every selected cell emptied. */
export function clearCells(targets: readonly Target[]): CellChange[] {
  return targets.map((t) => ({ ...t, text: "" }));
}
