// Display formats shared by server and client components: numbers, money, dates, lists
// and names as a locale writes them. Every function takes the locale (a translator's
// `locale`) and the store's time zone, never the runtime's own, so the server's render
// and the browser's agree. Server code gets a `Format` from `getFormat()`
// (`lib/get-format.ts`), client components from `useFormat()` (`lib/use-format.ts`).

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { withEnglish } from "@hyphacommerce/i18n";

import type { TFor } from "./messages";
import { DEFAULT_LANGUAGE, formatLocale } from "./pick-language.ts";

/**
 * A fixed locale for `Intl` used to compute (which year a time falls in, in a zone), never
 * to show: what's shown is written in the caller's locale.
 */
const ARITHMETIC = formatLocale(DEFAULT_LANGUAGE);

/** `Intl` formatters by kind, locale and options, built once: a table formats hundreds of cells. */
const formatters = new Map<string, unknown>();

function formatter<F>(kind: string, locale: string, options: object, make: () => F): F {
  const key = `${kind}:${locale}:${JSON.stringify(options)}`;
  if (!formatters.has(key)) formatters.set(key, make());
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- a key holds what its own `make` built
  return formatters.get(key) as F;
}

function numberFormat(locale: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  return formatter(
    "number",
    locale,
    options,
    () => new Intl.NumberFormat(withEnglish(locale), options),
  );
}

function dateFormat(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  return formatter(
    "date",
    locale,
    options,
    () => new Intl.DateTimeFormat(withEnglish(locale), options),
  );
}

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

/** "€18.00". Amounts are integers in minor units, never floats. `format.money(money)`. */
function formatMoney({ amount, currency }: Money, currencies: Currencies, locale: string): string {
  const digits = minorUnits(currencies, currency);
  return numberFormat(locale, {
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
 * 4 more), trailing zeros beyond the currency's dropped. `format.unitCost(cost)`.
 */
function formatUnitCost(cost: UnitCost, currencies: Currencies, locale: string): string {
  const digits = minorUnits(currencies, cost.currency);
  return numberFormat(locale, {
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
 * A time in `timeZone` (the store's). `long`: "15 Oct 2026, 09:30 CEST", for detail
 * pages. `short`, for tables: "15 Oct, 09:30" this year, "15 Oct 2025" before.
 * `<DateTime>` in JSX and `format.dateTime(value, style)` in text.
 */
function formatDateTime(
  rfc3339: string,
  timeZone: string,
  style: "long" | "short",
  locale: string,
): string {
  const at = new Date(rfc3339);
  if (Number.isNaN(at.getTime())) return "—";
  if (style === "long") {
    return dateFormat(locale, {
      day: "numeric",
      month: "short",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      timeZone,
      timeZoneName: "short",
    }).format(at);
  }
  // Which year a time falls in, in the store's zone: compared, never shown.
  const year = dateFormat(ARITHMETIC, { year: "numeric", timeZone });
  const thisYear = year.format(at) === year.format(new Date());
  return dateFormat(locale, {
    day: "numeric",
    month: "short",
    ...(thisYear ? { hour: "2-digit", minute: "2-digit" } : { year: "numeric" }),
    timeZone,
  }).format(at);
}

/**
 * A calendar date the API keeps as noon UTC (an expected date): the day in UTC, never the
 * store's zone, which would move it to the next day east of UTC+12. `long` "11 Nov 2026",
 * `short` "11 Nov".
 */
function formatCalendarDate(rfc3339: string, style: "long" | "short", locale: string): string {
  const at = new Date(rfc3339);
  if (Number.isNaN(at.getTime())) return "—";
  return dateFormat(locale, {
    day: "numeric",
    month: "short",
    ...(style === "long" ? { year: "numeric" } : {}),
    timeZone: "UTC",
  }).format(at);
}

/** "980 B", "1.4 MB": file sizes in powers of 1024, one decimal above a kilobyte. `format.bytes(bytes)`. */
function formatBytes(bytes: number, locale: string): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${value} B`;
  const number = numberFormat(locale, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
    useGrouping: false,
  }).format(value);
  return `${number} ${units[unit]}`;
}

/** The reasons an order is cancelled for, as the API names them, in the order offered. */
export const CANCEL_REASONS = [
  "customer",
  "inventory",
  "declined",
  "fraud",
  "staff",
  "other",
] as const;

/** Why an order was cancelled, as the admin says it (an unknown reason as it is). */
export function cancelReasonLabel(t: TFor<"orders">, reason: string): string {
  const known = CANCEL_REASONS.find((r) => r === reason);
  return known ? t(`orders.cancel_reason_${known}`) : reason;
}

/** "Ana Horvat", "Ana", or "" without a name. */
export function customerName(c: { first_name?: string | null; last_name?: string | null }): string {
  return [c.first_name, c.last_name].filter(Boolean).join(" ");
}

/**
 * Everything the admin formats, as one locale writes it. Made per request (`getFormat()`)
 * or per component (`useFormat()`) from the translator's `locale`, so call sites never
 * name one.
 */
export interface Format {
  /** The locale it formats for: the translator's. */
  readonly locale: string;
  /** "1,234.5". `options` as `Intl.NumberFormat` takes them. */
  number(value: number, options?: Intl.NumberFormatOptions): string;
  /** "12.5%" from 0.125: a fraction, at most two decimals unless `options` say otherwise. */
  percent(fraction: number, options?: Intl.NumberFormatOptions): string;
  /** "€18.00" from minor units. Throws for a currency the store's table lacks. */
  money(money: Money): string;
  /** "€4.125": a unit cost with the extra decimals it carries. */
  unitCost(cost: UnitCost): string;
  /**
   * A time from the API (RFC 3339) in the store's time zone: `long` "15 Oct 2026, 09:30
   * CEST", `short` "15 Oct, 09:30" this year and "15 Oct 2025" before. "—" for no date.
   */
  dateTime(rfc3339: string, style?: "long" | "short"): string;
  /** A calendar date kept as noon UTC, read in UTC: `long` "11 Nov 2026", `short` "11 Nov". */
  date(rfc3339: string, style?: "long" | "short"): string;
  /** "A, B and C" (`conjunction`, the default), "A, B or C" (`disjunction`), "A, B, C" (`unit`). */
  list(items: readonly string[], type?: Intl.ListFormatType): string;
  /** The plural category of `count` ("one", "few", "other"…). Messages use ICU plurals instead. */
  plural(count: number): Intl.LDMLPluralRule;
  /** "980 B", "1.4 MB". */
  bytes(bytes: number): string;
  /** "Croatia" for "HR"; the code itself when `Intl` has no name. */
  countryName(code: string): string;
  /** "Euro" for "EUR"; `undefined` when `Intl` has no name. */
  currencyName(code: string): string | undefined;
  /** "Serbian (Latin)" for "sr-Latn"; the tag itself when `Intl` has no name. */
  languageName(tag: string): string;
  /** Orders two strings as the locale sorts them, for `toSorted`. */
  compare(a: string, b: string): number;
}

/** What a `Format` needs besides the locale; both default to nothing known. */
export interface FormatContext {
  /** The currency table (`loadCurrencies`, `useCurrencies`): `money` and `unitCost` need it. */
  currencies?: Currencies;
  /** The store's time zone (`loadTimeZone`, `useTimeZone`); UTC without one. */
  timeZone?: string;
}

/**
 * `Intl.DisplayNames`' name for `code` in `locale`, or `undefined` for one it lacks or
 * can't read.
 */
export function displayName(
  type: "region" | "currency" | "language",
  code: string,
  locale: string,
): string | undefined {
  const names = formatter(
    "names",
    locale,
    { type },
    () => new Intl.DisplayNames(withEnglish(locale), { type, fallback: "none" }),
  );
  try {
    return names.of(code);
  } catch {
    // Not a well-formed code: the caller shows it as it is.
    return undefined;
  }
}

/** Orders two strings as `locale` sorts them. */
export function compareIn(locale: string, a: string, b: string): number {
  return formatter("collator", locale, {}, () => new Intl.Collator(withEnglish(locale))).compare(
    a,
    b,
  );
}

/**
 * A `Format` for `locale` (a translator's: `createFormat(t.locale, …)`). Its `Intl`
 * formatters are shared by every `Format` of the locale and built on first use.
 */
export function createFormat(locale: string, context: FormatContext = {}): Format {
  const { currencies = {}, timeZone = "UTC" } = context;
  return {
    locale,
    number: (value, options = {}) => numberFormat(locale, options).format(value),
    percent: (fraction, options = {}) =>
      numberFormat(locale, { style: "percent", maximumFractionDigits: 2, ...options }).format(
        fraction,
      ),
    money: (money) => formatMoney(money, currencies, locale),
    unitCost: (cost) => formatUnitCost(cost, currencies, locale),
    dateTime: (rfc3339, style = "long") => formatDateTime(rfc3339, timeZone, style, locale),
    date: (rfc3339, style = "long") => formatCalendarDate(rfc3339, style, locale),
    list: (items, type = "conjunction") =>
      formatter(
        "list",
        locale,
        { type },
        () => new Intl.ListFormat(withEnglish(locale), { style: "long", type }),
      ).format(items),
    plural: (count) =>
      formatter("plural", locale, {}, () => new Intl.PluralRules(withEnglish(locale))).select(
        count,
      ),
    bytes: (bytes) => formatBytes(bytes, locale),
    countryName: (code) => displayName("region", code, locale) ?? code,
    currencyName: (code) => displayName("currency", code, locale),
    languageName: (tag) => displayName("language", tag, locale) ?? tag,
    compare: (a, b) => compareIn(locale, a, b),
  };
}
