// Display formats shared by server and client components. Fixed locale and time zone,
// so the server's render and the browser's agree.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

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

type UnitCost = AdminSchemas["UnitCost"];

/** Extra decimals a unit cost keeps beyond the currency's minor unit. */
const COST_PLACES = 4;

/**
 * A unit cost's amount (a decimal string of minor units, "412.5") as a decimal string
 * of the currency's major unit ("4.125" for EUR), exactly: shifted as text, never through
 * a float.
 */
function costInMajorUnits({ amount, currency }: UnitCost, currencies: Currencies): string {
  const digits = minorUnits(currencies, currency);
  const [whole = "0", fraction = ""] = amount.split(".");
  const padded = whole.padStart(digits + 1, "0");
  const major = padded.slice(0, padded.length - digits).replace(/^0+(?=\d)/, "");
  const decimals = (padded.slice(padded.length - digits) + fraction).replace(/0+$/, "");
  return decimals ? `${major}.${decimals}` : major;
}

/**
 * "€4.125": a unit cost, with the currency's digits and the extra ones it carries (up to
 * 4 more), trailing zeros beyond the currency's dropped.
 */
export function formatUnitCost(cost: UnitCost, currencies: Currencies): string {
  const digits = minorUnits(currencies, cost.currency);
  return new Intl.NumberFormat("en-IE", {
    style: "currency",
    currency: cost.currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: digits + COST_PLACES,
  }).format(Number(costInMajorUnits(cost, currencies)));
}

/** The decimal form of a unit cost for an input ("4.125"). */
export function unitCostInput(cost: UnitCost, currencies: Currencies): string {
  return costInMajorUnits(cost, currencies);
}

/**
 * A unit cost's amount (minor units as a decimal string, as the API takes it) from what
 * someone typed in the major unit ("4.125" EUR is "412.5"), or null when it isn't a
 * non-negative amount with at most the currency's digits and 4 more. Shifted as text.
 */
export function parseUnitCost(
  input: string,
  currency: string,
  currencies: Currencies,
): string | null {
  const digits = minorUnits(currencies, currency);
  const match = /^(\d+)(?:[.,](\d*))?$/.exec(input.trim());
  if (!match) return null;
  const [, whole = "0", fraction = ""] = match;
  if (fraction.length > digits + COST_PLACES) return null;
  const minor = (whole + fraction.slice(0, digits).padEnd(digits, "0")).replace(/^0+(?=\d)/, "");
  const rest = fraction.slice(digits).replace(/0+$/, "");
  if (!Number.isSafeInteger(Number(minor))) return null;
  return rest ? `${minor}.${rest}` : minor;
}

/**
 * What `quantity` units cost at `cost`, in minor units rounded half away from zero, as
 * money: computed on integers (1/10,000 of a minor unit), never floats.
 */
export function unitCostTotal(cost: UnitCost, quantity: number): Money {
  const [whole = "0", fraction = ""] = cost.amount.split(".");
  const scaled = BigInt(whole + fraction.padEnd(COST_PLACES, "0"));
  const scale = 10n ** BigInt(COST_PLACES);
  const total = scaled * BigInt(quantity);
  const rounded = (total + scale / 2n) / scale;
  return { amount: Number(rounded), currency: cost.currency };
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
