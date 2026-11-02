// Stock as the admin counts it, shared by the home page and the inventory list.

/** Fewer available than this is "running low": the home page's count and the inventory view. */
export const LOW_STOCK = 5;

/** Why stock moved, as the history says it. */
export const REASON_LABELS: Record<string, string> = {
  received: "Received",
  transfer_out: "Transferred out",
  transfer_in: "Transferred in",
  sold: "Sold",
  restocked: "Restocked",
  counted: "Counted",
  correction: "Correction",
  damaged: "Damaged",
  shrinkage: "Lost or stolen",
  promotion: "Sample or gift",
  other: "Other",
  cost_adjustment: "Cost changed",
  opening: "Opening count",
};

/** Every reason, as the history filter offers them. */
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

/** The reasons a person gives for adding or removing units by hand. */
export const ADJUSTMENT_REASONS = [
  "correction",
  "damaged",
  "shrinkage",
  "promotion",
  "other",
] as const;

export type AdjustmentReason = (typeof ADJUSTMENT_REASONS)[number];

/**
 * What a level has on its way: "5 incoming", or "5 incoming, expected 1 Mar 2027" when an
 * order says when. `null` when nothing is coming. Expected dates are days, chosen as noon
 * UTC, so they're read in UTC.
 */
export function incomingLabel(level: {
  incoming: number;
  next_expected_at?: string | null;
}): string | null {
  if (level.incoming <= 0) return null;
  if (!level.next_expected_at) return `${level.incoming} incoming`;
  const day = new Intl.DateTimeFormat("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(level.next_expected_at));
  return `${level.incoming} incoming, expected ${day}`;
}
