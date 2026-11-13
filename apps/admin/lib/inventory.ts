// Stock as the admin counts it, shared by the home page and the inventory list.

import type { Format } from "./format";
import type { TFor } from "./messages";

/** Fewer available than this is "running low": the home page's count and the inventory view. */
export const LOW_STOCK = 5;

/** Every reason stock moved, as the history filter offers them. */
export const STOCK_REASONS = [
  "received",
  "transfer_out",
  "transfer_in",
  "sold",
  "restocked",
  "counted",
  "correction",
  "damaged",
  "shrinkage",
  "promotion",
  "other",
  "cost_adjustment",
  "opening",
] as const;

/** Why stock moved, as the history says it (an unknown reason as it is). */
export function reasonLabel(t: TFor<"inventory">, reason: string): string {
  const known = STOCK_REASONS.find((r) => r === reason);
  return known ? t(`inventory.reason_${known}`) : reason;
}

/** The reasons a person gives for adding or removing units by hand. */
export const ADJUSTMENT_REASONS = [
  "correction",
  "damaged",
  "shrinkage",
  "promotion",
  "other",
] as const;

export type AdjustmentReason = (typeof ADJUSTMENT_REASONS)[number];

type Level = { incoming: number; next_expected_at?: string | null };

/** @deprecated Names no translator, so it writes English: pass `t` and a `Format`. Deleted once every area is extracted. */
export function incomingLabel(level: Level): string | null;
/**
 * What a level has on its way: "5 incoming", or "5 incoming, expected 1 Mar 2027" when an
 * order says when. `null` when nothing is coming. Expected dates are days, chosen as noon
 * UTC, so they're read in UTC (`format.date`).
 */
export function incomingLabel(level: Level, t: TFor<"inventory">, format: Format): string | null;
export function incomingLabel(level: Level, t?: TFor<"inventory">, format?: Format): string | null {
  if (level.incoming <= 0) return null;
  if (!t || !format) {
    // The products page's call, until it passes a translator: English, as before.
    if (!level.next_expected_at) return `${level.incoming} incoming`;
    const day = new Intl.DateTimeFormat("en-GB", {
      day: "numeric",
      month: "short",
      year: "numeric",
      timeZone: "UTC",
    }).format(new Date(level.next_expected_at));
    return `${level.incoming} incoming, expected ${day}`;
  }
  if (!level.next_expected_at) return t("inventory.incoming_count", { count: level.incoming });
  return t("inventory.incoming_count_expected", {
    count: level.incoming,
    date: format.date(level.next_expected_at),
  });
}
