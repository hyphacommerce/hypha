// Applying a CSV import in steps. The file sits in storage (an upload) and every step
// reads it again and matches it against the list's prices as they are then, so no
// diff travels between the browser and the server. A step is a range of the file's
// lines, fixed by the preview: rows an earlier step applied read as unchanged
// afterwards, but they sit outside later steps' ranges, so each step still sends
// exactly its own rows' changes.

import type { ImportRow } from "./price-csv.ts";
import type { PriceGridDiff } from "./price-grid/types.ts";

/** The largest file a `price_import` upload takes; the API refuses a larger one. */
export const MAX_IMPORT_BYTES = 20 * 1024 * 1024;

/** One step of applying: the file's lines `from` to `to` (both included). */
export type ImportStep = {
  from: number;
  to: number;
  /** How many changes the preview found on those lines. */
  changes: number;
};

/** A row the API refused while applying. */
export type ImportFailure = { line?: number; variant: string; error: string };

/**
 * The steps applying `rows` takes, each holding at most `size` of their changes, in the
 * file's order. Unchanged, unmatched and invalid rows send nothing, so they don't count.
 */
export function importSteps(rows: readonly ImportRow[], size: number): ImportStep[] {
  const steps: ImportStep[] = [];
  let current: ImportStep | undefined;
  for (const row of rows) {
    if (row.status !== "changed") continue;
    if (!current || current.changes >= size) {
      current = { from: row.line, to: row.line, changes: 0 };
      steps.push(current);
    }
    current.to = row.line;
    current.changes++;
  }
  return steps;
}

/** The part of `diff` that `step`'s lines make, per the `rows` it was made with. */
export function stepDiff(
  rows: readonly ImportRow[],
  diff: PriceGridDiff,
  step: Pick<ImportStep, "from" | "to">,
): PriceGridDiff {
  const inStep = new Set(
    rows.flatMap((r) =>
      r.status === "changed" && r.variant && r.line >= step.from && r.line <= step.to
        ? [r.variant]
        : [],
    ),
  );
  return {
    upserts: diff.upserts.filter((u) => inStep.has(u.variant)),
    clears: diff.clears.filter((c) => inStep.has(c.variant)),
  };
}

/**
 * The rows of `diff` that a write refused: each refused variant with its line in the
 * file, or every one of them with the write's message when it failed whole.
 */
export function stepFailures(
  rows: readonly ImportRow[],
  diff: PriceGridDiff,
  result: { error?: string; rowErrors?: Record<string, string> },
): ImportFailure[] {
  // The changed row's line: a later row naming the same variant is invalid and sent nothing.
  const lineOf = new Map(
    rows.flatMap((r) =>
      r.status === "changed" && r.variant ? [[r.variant, r.line] as const] : [],
    ),
  );
  if (result.rowErrors) {
    return Object.entries(result.rowErrors).map(([variant, error]) => ({
      variant,
      line: lineOf.get(variant),
      error,
    }));
  }
  const { error } = result;
  if (!error) return [];
  return [...diff.upserts, ...diff.clears].map(({ variant }) => ({
    variant,
    line: lineOf.get(variant),
    error,
  }));
}
