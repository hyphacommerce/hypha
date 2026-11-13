// Quantity price breaks: their rules, and the one-line text a grid cell holds for them
// ("10=8.00; 20=7.50"), so breaks copy, paste and fill like any other cell.

import { formatAmount, parseAmount, type Parsed } from "./money.ts";
import { say } from "./say.ts";
import { MAX_BREAKS, type PriceT } from "./types.ts";

/** A break in minor units of the list's currency. */
export type BreakAmount = { min_quantity: number; amount: number };

/** "10=8.00; 20=7.50"; "" for none. */
export function formatBreaks(breaks: readonly BreakAmount[], digits: number): string {
  return breaks.map((b) => `${b.min_quantity}=${formatAmount(b.amount, digits)}`).join("; ");
}

/**
 * Breaks from their cell text: pairs separated by `;` or new lines, each a quantity and
 * a price joined by `=`, `:` or `@` ("10 @ 8,00" works too). Their rules are checked
 * apart, by `breakProblems`. The error is in `t`'s language (English without one, for a
 * caller that only asks whether the text parses).
 */
export function parseBreaks(
  text: string,
  digits: number,
  currency: string,
  t?: PriceT,
): Parsed<BreakAmount[]> {
  const parts = text
    .split(/[;\n]/)
    .map((part) => part.trim())
    .filter((part) => part !== "");
  const breaks: BreakAmount[] = [];
  for (const part of parts) {
    const match = /^(\d+)\s*[=:@]\s*(.+)$/.exec(part);
    if (!match) {
      return {
        ok: false,
        error: say(t, "price_lists.break_invalid", { part: JSON.stringify(part) }),
      };
    }
    const [, quantity = "", price = ""] = match;
    const amount = parseAmount(price, digits, currency, t);
    if (!amount.ok) {
      return {
        ok: false,
        error: say(t, "price_lists.break_amount_invalid", { quantity, error: amount.error }),
      };
    }
    breaks.push({ min_quantity: Number(quantity), amount: amount.value });
  }
  return { ok: true, value: breaks };
}

/**
 * Every rule the breaks break, as the API states them: at most 10, quantities from 2 up
 * and strictly increasing, each price below the one before it, the first below the fixed
 * price (which must exist). Empty when they're fine.
 */
export function breakProblems(
  t: PriceT,
  breaks: readonly BreakAmount[],
  fixedAmount: number | null,
): string[] {
  if (breaks.length === 0) return [];
  const problems: string[] = [];
  if (fixedAmount === null) problems.push(t("price_lists.breaks_need_fixed_price"));
  if (breaks.length > MAX_BREAKS)
    problems.push(t("price_lists.breaks_too_many", { max: MAX_BREAKS }));
  let previousQuantity = 1;
  let previousAmount = fixedAmount;
  for (const [index, b] of breaks.entries()) {
    if (!Number.isSafeInteger(b.min_quantity) || b.min_quantity < 2) {
      problems.push(t("price_lists.break_quantity_too_low", { quantity: b.min_quantity }));
    } else if (b.min_quantity <= previousQuantity) {
      problems.push(
        t("price_lists.break_quantity_not_increasing", {
          quantity: b.min_quantity,
          previous: previousQuantity,
        }),
      );
    }
    if (previousAmount !== null && b.amount >= previousAmount) {
      problems.push(
        index === 0
          ? t("price_lists.break_price_not_below_fixed", { quantity: b.min_quantity })
          : t("price_lists.break_price_not_below_previous", { quantity: b.min_quantity }),
      );
    }
    previousQuantity = Math.max(previousQuantity, b.min_quantity);
    previousAmount = b.amount;
  }
  return problems;
}

/** "1 break", "3 breaks". */
export function breaksLabel(t: PriceT, count: number): string {
  return t("price_lists.breaks_count", { count });
}
