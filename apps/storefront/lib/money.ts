// Money as shoppers read it. Amounts are integers in minor units; how many digits
// a currency has comes from `/storefront/countries`, never assumed (1050 is 10.50
// EUR but 1050 JPY). How it's written is the page's locale's (a translator's `locale`):
// "€10.50" in `en-IE`, "10,50 €" in `de-DE`.

import { withEnglish } from "./format.ts";

export type Money = { amount: number; currency: string };

/** "€10.50". `digits` is the currency's minor units. */
export function formatMoney({ amount, currency }: Money, digits: number, locale: string): string {
  return new Intl.NumberFormat(withEnglish(locale), {
    style: "currency",
    currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(amount / 10 ** digits);
}

/** "€9.00", or "€9.00 – €14.00" when the range is a range. */
export function formatRange(min: Money, max: Money, digits: number, locale: string): string {
  return min.amount === max.amount
    ? formatMoney(min, digits, locale)
    : `${formatMoney(min, digits, locale)} – ${formatMoney(max, digits, locale)}`;
}
