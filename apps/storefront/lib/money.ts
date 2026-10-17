// Money as shoppers read it (design note 0023, SF6). Amounts are integers in minor
// units; how many digits a currency has comes from `/storefront/countries`, never
// assumed (1050 is 10.50 EUR but 1050 JPY).

export type Money = { amount: number; currency: string };

/** "€10.50". `digits` is the currency's minor units. */
export function formatMoney({ amount, currency }: Money, digits: number, locale = "en"): string {
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(amount / 10 ** digits);
}

/** "€9.00", or "€9.00 – €14.00" when the range is a range. */
export function formatRange(min: Money, max: Money, digits: number): string {
  return min.amount === max.amount
    ? formatMoney(min, digits)
    : `${formatMoney(min, digits)} – ${formatMoney(max, digits)}`;
}
