// Display formats shared by server and client components. Fixed locale and time zone,
// so the server's render and the browser's agree.

import type { AdminSchemas } from "@hypha/sdk/admin";

type Money = AdminSchemas["Money"];

/**
 * Minor-unit digits by currency code, as `GET /admin/currencies` lists them: 1050 is
 * 10.50 EUR but 1050 JPY. Server code gets it from `loadCurrencies`, client components
 * from `useCurrencies`. A plain object, so a server page can pass it to a client
 * component.
 */
export type Currencies = Readonly<Record<string, number>>;

/** Every currency code, sorted, for pickers. */
export function currencyCodes(currencies: Currencies): string[] {
  return Object.keys(currencies).toSorted();
}

/**
 * The digits of `currency`. ISO's, not `Intl`'s: they differ for a few (HUF, IDR, IQD…),
 * and only ISO's say what a stored amount means. An unknown one throws rather than guess,
 * since a wrong guess misstates the amount by a power of ten.
 */
function minorUnits(currencies: Currencies, currency: string): number {
  const digits = currencies[currency];
  if (digits === undefined) throw new Error(`unknown currency ${JSON.stringify(currency)}`);
  return digits;
}

/** "€18.00". Amounts are integers in minor units, never floats. */
export function formatMoney({ amount, currency }: Money, currencies: Currencies): string {
  const digits = minorUnits(currencies, currency);
  return new Intl.NumberFormat("en-IE", {
    style: "currency",
    currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(amount / 10 ** digits);
}

/** The decimal form of a money amount for an input ("18.00"). */
export function moneyInput({ amount, currency }: Money, currencies: Currencies): string {
  const digits = minorUnits(currencies, currency);
  return (amount / 10 ** digits).toFixed(digits);
}

/**
 * Minor units from what someone typed ("18", "18.5", "18.50"), or null when it isn't a
 * non-negative amount with at most the currency's digits. Parsed as text, so 0.1 + 0.2
 * style float errors can't creep in.
 */
export function parseMoney(input: string, currency: string, currencies: Currencies): number | null {
  const digits = minorUnits(currencies, currency);
  const match = /^(\d+)(?:[.,](\d*))?$/.exec(input.trim());
  if (!match) return null;
  const [, whole, fraction = ""] = match;
  if (fraction.length > digits) return null;
  const amount = Number(whole + fraction.padEnd(digits, "0"));
  return Number.isSafeInteger(amount) ? amount : null;
}

/**
 * A time in `timeZone` (the store's; `components/date-time.tsx` passes it). `long`:
 * "15 Oct 2026, 09:30 CEST", for detail pages. `short`, for tables: "15 Oct, 09:30" this
 * year, "15 Oct 2025" before.
 */
export function formatDateTime(
  rfc3339: string,
  timeZone = "UTC",
  style: "long" | "short" = "long",
): string {
  const at = new Date(rfc3339);
  if (Number.isNaN(at.getTime())) return "—";
  if (style === "long") {
    return new Intl.DateTimeFormat("en-GB", {
      day: "numeric",
      month: "short",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      timeZone,
      timeZoneName: "short",
    }).format(at);
  }
  const year = (d: Date) =>
    new Intl.DateTimeFormat("en-GB", { year: "numeric", timeZone }).format(d);
  const thisYear = year(at) === year(new Date());
  return new Intl.DateTimeFormat("en-GB", {
    day: "numeric",
    month: "short",
    ...(thisYear ? { hour: "2-digit", minute: "2-digit" } : { year: "numeric" }),
    timeZone,
  }).format(at);
}

/** "980 B", "1.4 MB": file sizes in powers of 1024, one decimal above a kilobyte. */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${value} B` : `${value.toFixed(1)} ${units[unit]}`;
}

/** Why an order was cancelled, as the admin says it (an unknown reason as it is). */
export const CANCEL_REASONS: Record<string, string> = {
  customer: "The customer asked",
  inventory: "Out of stock or can't ship",
  declined: "Payment declined",
  fraud: "Fraud",
  staff: "Staff error",
  other: "Other",
};

/** "Ana Horvat", "Ana", or "" without a name. */
export function customerName(c: { first_name?: string | null; last_name?: string | null }): string {
  return [c.first_name, c.last_name].filter(Boolean).join(" ");
}
