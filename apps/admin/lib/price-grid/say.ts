// The messages of the helpers other areas still call without a translator (amounts and
// percentages in the discount forms, a phase's badge in the discounts table, a candidate
// list's reason on the product page). With a translator they come from the catalog like
// any other; without one they're the English below, word for word what the catalog says
// (`say.test.ts` holds the two together).
//
// LEGACY: once every caller passes its translator, make `t` required in `parseAmount`,
// `parsePercent`, `parseBreaks`, `phaseBadge` and `candidateReason`, and delete this
// table.

import type { KeyIn } from "../messages";
import type { PriceT } from "./types.ts";

type Values = Readonly<Record<string, string | number>>;

const ENGLISH = {
  "price_lists.amount_foreign_currency": (v) =>
    `That's in ${v.written}; this list is in ${v.currency}.`,
  "price_lists.amount_required": () => "Enter an amount.",
  "price_lists.amount_negative": () => "Can't be negative.",
  "price_lists.amount_ambiguous": (v) =>
    `${v.input} is ambiguous: write it without the separator, or with ${v.digits} ${v.digits === 1 ? "decimal" : "decimals"}.`,
  "price_lists.amount_no_decimals": () => "This currency has no decimals.",
  "price_lists.amount_too_many_decimals": (v) =>
    `At most ${v.digits} ${v.digits === 1 ? "decimal place" : "decimal places"}.`,
  "price_lists.amount_too_large": () => "Too large.",
  "price_lists.amount_invalid": (v) => `${v.input} isn't an amount.`,
  "price_lists.percent_invalid": () => "A percentage, like -10 or 5.5.",
  "price_lists.percent_too_many_decimals": () => "At most two decimal places.",
  "price_lists.percent_too_large": () => "Too large.",
  "price_lists.percent_below_minimum": () => "Can't take off more than 100%.",
  "price_lists.percent_above_maximum": () => "At most +10,000%.",
  "price_lists.break_invalid": (v) => `${v.part} isn't a break, like 10=8.00.`,
  "price_lists.break_amount_invalid": (v) => `Break ${v.quantity}: ${v.error}`,
  "price_lists.phase_active": () => "Active",
  "price_lists.phase_scheduled": () => "Scheduled",
  "price_lists.phase_expired": () => "Expired",
  "price_lists.phase_draft": () => "Draft",
  "price_lists.candidate_won": () => "Wins: the lowest price.",
  "price_lists.candidate_higher": () => "Loses: its price is higher.",
  "price_lists.candidate_tied_with_older": () =>
    "Loses: the same price as an older list, which wins the tie.",
  "price_lists.candidate_not_live_scheduled": () => "Not live yet: its window hasn't started.",
  "price_lists.candidate_not_live_expired": () => "Not live: its window ended.",
  "price_lists.candidate_not_live_draft": () => "Not live: it's a draft.",
  "price_lists.candidate_not_pricing": () =>
    "Doesn't price it: no fixed price, and its adjustment doesn't cover the product.",
  "price_lists.candidate_not_in_audience": () =>
    "Not for this buyer: it prices only for other customer groups.",
} satisfies Partial<Record<KeyIn<"price_lists">, (values: Values) => string>>;

/** The messages a caller without a translator can still get. */
export type LegacyKey = keyof typeof ENGLISH;

/** Every legacy key, for the test that holds the table to the catalog. */
// oxlint-disable-next-line typescript/no-unsafe-type-assertion -- an object's own keys
export const LEGACY_KEYS = Object.keys(ENGLISH) as LegacyKey[];

/** The message under `key`: `t`'s, or today's English for a caller that has no translator yet. */
export function say(t: PriceT | undefined, key: LegacyKey, values: Values = {}): string {
  return t ? t(key, values) : ENGLISH[key](values);
}
