// Display formats shared by server and client components. Fixed locale and time zone,
// so the server's render and the browser's agree.

import type { AdminSchemas } from "@hypha/sdk/admin";

type Money = AdminSchemas["Money"];

/**
 * Minor-unit digits per currency (1050 is 10.50 EUR). Typed by the API's currency list,
 * so a currency added there fails to compile here until it has an entry.
 */
const MINOR_DIGITS: Record<Money["currency"], number> = {
  EUR: 2,
  USD: 2,
  RSD: 2,
};

/** Every currency the API takes, for pickers. */
export const CURRENCIES = Object.keys(MINOR_DIGITS) as Money["currency"][];

/** "€18.00". Amounts are integers in minor units, never floats (CLAUDE.md, Money). */
export function formatMoney({ amount, currency }: Money): string {
  const digits = MINOR_DIGITS[currency];
  return new Intl.NumberFormat("en-IE", {
    style: "currency",
    currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(amount / 10 ** digits);
}

/** The decimal form of a money amount for an input ("18.00"). */
export function moneyInput({ amount, currency }: Money): string {
  return (amount / 10 ** MINOR_DIGITS[currency]).toFixed(MINOR_DIGITS[currency]);
}

/**
 * Minor units from what someone typed ("18", "18.5", "18.50"), or null when it isn't a
 * non-negative amount with at most the currency's digits. Parsed as text, so 0.1 + 0.2
 * style float errors can't creep in.
 */
export function parseMoney(input: string, currency: Money["currency"]): number | null {
  const digits = MINOR_DIGITS[currency];
  const match = /^(\d+)(?:[.,](\d*))?$/.exec(input.trim());
  if (!match) return null;
  const [, whole, fraction = ""] = match;
  if (fraction.length > digits) return null;
  const amount = Number(whole + fraction.padEnd(digits, "0"));
  return Number.isSafeInteger(amount) ? amount : null;
}

/** "15 Oct 2026, 09:30 UTC". */
export function formatDateTime(rfc3339: string): string {
  return `${new Intl.DateTimeFormat("en-GB", {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: "UTC",
  }).format(new Date(rfc3339))} UTC`;
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
