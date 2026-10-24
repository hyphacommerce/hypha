// Writing a grid's diff (a save or an import step) through the fixed prices routes. The
// requests come in as functions, so this runs under `node --test` with fakes.

import { diffRequests } from "../price-csv.ts";
import { entryErrors } from "../price-lists.ts";
import type { FixedPriceUpsert, PriceGridDiff } from "./types.ts";

/** An API error as the SDK answers it. */
export type WriteError = { code: string; error: string; details?: unknown };

export type DiffWriter = {
  /** `PUT …/fixed-prices` with these entries. */
  put: (entries: readonly FixedPriceUpsert[]) => Promise<{ error?: WriteError }>;
  /** `DELETE …/fixed-prices` for these variants. */
  remove: (variants: string[]) => Promise<{ error?: WriteError }>;
  /** The message shown for an error. */
  describe: (error: WriteError) => string;
};

/** What a write answers: the grid's `PriceGridSaveResult`. */
export type SaveResult = { error?: string; rowErrors?: Record<string, string> };

/**
 * Writes `diff`: a `PUT` per 1,000 fixed prices and a `DELETE` per 1,000 cleared ones.
 * A `PUT` is all or nothing, so one refused for some entries is sent again without
 * them, and only those keep their drafts. Every request is tried, so one failing
 * doesn't hold up the rest. One at a time, on purpose: each can carry 1,000 prices,
 * and the API checks each request whole.
 */
export async function writeDiff(diff: PriceGridDiff, writer: DiffWriter): Promise<SaveResult> {
  const { put, remove, describe } = writer;
  const { upserts, clears } = diffRequests(diff);
  const rowErrors: Record<string, string> = {};
  let error: string | undefined;
  for (const entries of upserts) {
    const first = await put(entries);
    if (!first.error) continue;
    const refused =
      first.error.code === "fixed_prices_invalid"
        ? entryErrors(first.error.details, entries)
        : null;
    if (!refused || Object.keys(refused).length === 0) {
      for (const e of entries) rowErrors[e.variant] = describe(first.error);
      error ??= describe(first.error);
      continue;
    }
    Object.assign(rowErrors, refused);
    const rest = entries.filter((e) => !(e.variant in refused));
    if (rest.length === 0) continue;
    const second = await put(rest);
    if (second.error) {
      const again =
        second.error.code === "fixed_prices_invalid"
          ? entryErrors(second.error.details, rest)
          : null;
      for (const e of rest) rowErrors[e.variant] = again?.[e.variant] ?? describe(second.error);
    }
  }
  for (const variants of clears) {
    const { error: e } = await remove(variants);
    if (e) {
      for (const v of variants) rowErrors[v] = describe(e);
      error ??= describe(e);
    }
  }
  return Object.keys(rowErrors).length > 0 ? { error, rowErrors } : {};
}
