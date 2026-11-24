// What a price is shown as: struck-through compare-at prices ("was"), quantity breaks
// and the words a screen reader gets for them. Pure, so servers and browsers share it.

import type { TFor } from "./messages.ts";
import { type Money, formatMoney } from "./money.ts";

export type PriceBreak = { min_quantity: number; price: Money };

/** The compare-at price when it's worth striking through: above the price, in its
 * currency. The API sends one only then; this keeps a partial read honest. */
export function shownCompareAt(price: Money, compareAt: Money | null | undefined): Money | null {
  if (!compareAt || compareAt.currency !== price.currency) return null;
  return compareAt.amount > price.amount ? compareAt : null;
}

/** What one costs when `quantity` go in one cart line: the break with the highest
 * `min_quantity` reached, else `price`. The cart prices the line the same way. */
export function unitPriceFor(price: Money, breaks: readonly PriceBreak[], quantity: number): Money {
  let best: PriceBreak | undefined;
  for (const b of breaks) {
    if (b.min_quantity <= quantity && (!best || b.min_quantity > best.min_quantity)) best = b;
  }
  return best?.price ?? price;
}

/** The breaks as a table lists them, from the smallest quantity. */
export function sortedBreaks(breaks: readonly PriceBreak[]): PriceBreak[] {
  return breaks.toSorted((a, b) => a.min_quantity - b.min_quantity);
}

/** "Buy 10+: €8.00 each". */
export function breakLabel(t: TFor<"product">, b: PriceBreak, digits: number): string {
  return t("product.price_break", {
    quantity: b.min_quantity,
    price: formatMoney(b.price, digits, t.locale),
  });
}

/** What a screen reader hears for a struck-through price: "Was €40.00, now €30.00". */
export function saleLabel(t: TFor<"common">, was: string, now: string): string {
  return t("common.was_now", { was, now });
}

/** A cart or order line's compare-at total: the unit compare-at times the quantity, shown only
 * when it's above what the line costs. */
export function lineCompareAt(
  compareAt: Money | null | undefined,
  quantity: number,
  total: Money,
): Money | null {
  if (!compareAt) return null;
  const amount = compareAt.amount * quantity;
  if (!Number.isSafeInteger(amount)) return null;
  return shownCompareAt(total, { amount, currency: compareAt.currency });
}
