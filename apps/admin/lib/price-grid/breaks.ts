// Quantity price breaks: their rules, and the one-line text a grid cell holds for them
// ("10=8.00; 20=7.50"), so breaks copy, paste and fill like any other cell.

import { formatAmount, parseAmount, type Parsed } from "./money.ts";
import { MAX_BREAKS } from "./types.ts";

/** A break in minor units of the list's currency. */
export type BreakAmount = { min_quantity: number; amount: number };

/** "10=8.00; 20=7.50"; "" for none. */
export function formatBreaks(breaks: readonly BreakAmount[], digits: number): string {
  return breaks.map((b) => `${b.min_quantity}=${formatAmount(b.amount, digits)}`).join("; ");
}

/**
 * Breaks from their cell text: pairs separated by `;` or new lines, each a quantity and
 * a price joined by `=`, `:` or `@` ("10 @ 8,00" works too). Their rules are checked
 * apart, by `breakProblems`.
 */
export function parseBreaks(text: string, digits: number): Parsed<BreakAmount[]> {
  const parts = text
    .split(/[;\n]/)
    .map((part) => part.trim())
    .filter((part) => part !== "");
  const breaks: BreakAmount[] = [];
  for (const part of parts) {
    const match = /^(\d+)\s*[=:@]\s*(.+)$/.exec(part);
    if (!match) {
      return { ok: false, error: `${JSON.stringify(part)} isn't a break, like 10=8.00.` };
    }
    const [, quantity = "", price = ""] = match;
    const amount = parseAmount(price, digits);
    if (!amount.ok) return { ok: false, error: `Break ${quantity}: ${amount.error}` };
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
  breaks: readonly BreakAmount[],
  fixedAmount: number | null,
): string[] {
  if (breaks.length === 0) return [];
  const problems: string[] = [];
  if (fixedAmount === null) problems.push("Breaks need a fixed price.");
  if (breaks.length > MAX_BREAKS) problems.push(`At most ${MAX_BREAKS} breaks.`);
  let previousQuantity = 1;
  let previousAmount = fixedAmount;
  for (const [index, b] of breaks.entries()) {
    if (!Number.isSafeInteger(b.min_quantity) || b.min_quantity < 2) {
      problems.push(`Quantities start at 2 (${b.min_quantity}).`);
    } else if (b.min_quantity <= previousQuantity) {
      problems.push(`Quantities must increase (${b.min_quantity} after ${previousQuantity}).`);
    }
    if (previousAmount !== null && b.amount >= previousAmount) {
      problems.push(
        index === 0
          ? `The price from ${b.min_quantity} must be below the fixed price.`
          : `The price from ${b.min_quantity} must be below the one before it.`,
      );
    }
    previousQuantity = Math.max(previousQuantity, b.min_quantity);
    previousAmount = b.amount;
  }
  return problems;
}

/** "1 break", "3 breaks". */
export function breaksLabel(count: number): string {
  return count === 1 ? "1 break" : `${count} breaks`;
}
