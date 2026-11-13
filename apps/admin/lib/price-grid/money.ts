// Money as the grid and the CSV files type it: decimal text in and out, integer minor
// units in between. Nothing here goes through a float, so "0.29" is 29 cents, never
// 28.999….
//
// None of it follows the reader's language: what's typed, pasted from a spreadsheet or
// read from a file means the same amount for everyone ("." and "," are both accepted,
// and what's written back is always plain decimals with a "."). Only the messages are
// translated.

import { type PriceT, say } from "./say.ts";

/** The largest amount the API takes either way (2⁵³ − 1), so JSON keeps it exact. */
export const MAX_AMOUNT = 2 ** 53 - 1;

/** The lowest adjustment there is: −100%. */
export const MIN_BASIS_POINTS = -10_000;
/** The highest a typed adjustment goes: +10,000%. */
export const MAX_BASIS_POINTS = 1_000_000;

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

const ok = <T>(value: T): Parsed<T> => ({ ok: true, value });
const fail = <T>(error: string): Parsed<T> => ({ ok: false, error });

// Spaces (incl. no-break and narrow no-break ones) and apostrophes ("1'234.50", Swiss)
// only ever group thousands.
const GROUPING = /[\s  ']/g;
// A code or a currency sign at either end ("EUR 12.50", "12,50 €", "$12").
const LEADING = /^([A-Za-z]{3}|\p{Sc})\s*/u;
const TRAILING = /\s*([A-Za-z]{3}|\p{Sc})$/u;

let knownCodes: ReadonlySet<string> | undefined;

/** ISO 4217 codes the runtime knows, so "USD" is a currency and "abc" isn't. */
function isCurrencyCode(code: string): boolean {
  if (!knownCodes) {
    try {
      knownCodes = new Set(Intl.supportedValuesOf("currency"));
    } catch {
      knownCodes = new Set();
    }
  }
  return knownCodes.has(code);
}

/**
 * How `currency` is written in English ("€", "$", "CA$"), longest first. English whatever
 * the reader's language: these are the signs stripped from typed and pasted amounts, and
 * the same text must parse alike for everyone.
 */
function symbolsOf(currency: string): string[] {
  const symbols = new Set<string>();
  for (const currencyDisplay of ["symbol", "narrowSymbol"] as const) {
    try {
      const part = new Intl.NumberFormat("en", { style: "currency", currency, currencyDisplay })
        .formatToParts(0)
        .find((p) => p.type === "currency");
      if (part && part.value !== currency) symbols.add(part.value);
    } catch {
      // A code the runtime doesn't know has no symbol to strip.
    }
  }
  return [...symbols].toSorted((a, b) => b.length - a.length);
}

/**
 * `text` without `currency`'s code or symbol at its ends. Another currency's code or any
 * other currency sign is an error: "$12" in a EUR list is a mistake, not 12 euros.
 */
function stripCurrency(text: string, currency: string, t?: PriceT): Parsed<string> {
  const symbols = symbolsOf(currency);
  const foreign = (written: string) =>
    fail<string>(say(t, "price_lists.amount_foreign_currency", { written, currency }));
  let rest = text;
  for (const end of ["start", "end"] as const) {
    const own = symbols.find((s) => (end === "start" ? rest.startsWith(s) : rest.endsWith(s)));
    if (own) {
      rest = (end === "start" ? rest.slice(own.length) : rest.slice(0, -own.length)).trim();
      continue;
    }
    const match = (end === "start" ? LEADING : TRAILING).exec(rest);
    const written = match?.[1];
    if (!match || written === undefined) continue;
    const code = written.toUpperCase();
    if (/^\p{Sc}$/u.test(written)) return foreign(written);
    if (code !== currency) {
      if (isCurrencyCode(code)) return foreign(code);
      // Three letters that aren't a currency are left for the amount check to refuse.
      continue;
    }
    rest = end === "start" ? rest.slice(match[0].length) : rest.slice(0, match.index);
  }
  return ok(rest);
}

/**
 * Minor units from an amount as people type or spreadsheets export it: "12.5", "12,50",
 * "1 234,50", "1,234.50", "1.234,50", "1,234,567", "€12.50". A lone separator is the
 * decimal point, except that "1,234" (three digits the currency doesn't have) is refused
 * as ambiguous; in KWD, with three digits, it's 1.234. Negative amounts and more decimals
 * than the currency has are refused, never rounded. `currency`'s code or symbol may stand
 * at either end; another currency's is refused. The error is in `t`'s language (English
 * without one).
 */
export function parseAmount(
  input: string,
  digits: number,
  currency: string,
  t?: PriceT,
): Parsed<number> {
  const garbage = () =>
    fail<number>(say(t, "price_lists.amount_invalid", { input: JSON.stringify(input.trim()) }));
  const stripped = stripCurrency(input.trim(), currency, t);
  if (!stripped.ok) return stripped;
  const text = stripped.value.replaceAll(GROUPING, "");
  if (text === "") {
    return input.trim() === "" ? fail(say(t, "price_lists.amount_required")) : garbage();
  }
  if (/^[-−]/.test(text)) return fail(say(t, "price_lists.amount_negative"));
  if (!/^\+?[\d.,]+$/.test(text) || !/\d/.test(text)) return garbage();
  const body = text.replace(/^\+/, "");

  const lastDot = body.lastIndexOf(".");
  const lastComma = body.lastIndexOf(",");
  let whole: string;
  let fraction: string;
  if (lastDot >= 0 && lastComma >= 0) {
    // Both: the later one is the decimal point, the other groups.
    const point = Math.max(lastDot, lastComma);
    const group = point === lastDot ? "," : ".";
    const head = body.slice(0, point);
    fraction = body.slice(point + 1);
    if (head.includes(body.charAt(point)) || !isGrouped(head, group)) return garbage();
    whole = head.replaceAll(group, "");
  } else if (lastDot >= 0 || lastComma >= 0) {
    const separator = lastDot >= 0 ? "." : ",";
    const parts = body.split(separator);
    const tail = parts.at(-1) ?? "";
    if (
      parts.length === 2 &&
      tail.length === 3 &&
      digits < 3 &&
      /^[1-9]\d{0,2}$/.test(parts[0] ?? "")
    ) {
      // "1,234" is a thousand to some and one and a bit to others: a price can't guess.
      return fail(
        say(t, "price_lists.amount_ambiguous", { input: JSON.stringify(input.trim()), digits }),
      );
    }
    if (parts.length > 2) {
      if (!isGrouped(body, separator)) return garbage();
      whole = parts.join("");
      fraction = "";
    } else {
      whole = parts[0] ?? "";
      fraction = tail;
    }
  } else {
    whole = body;
    fraction = "";
  }

  if (!/^\d*$/.test(whole) || !/^\d*$/.test(fraction) || whole + fraction === "") {
    return garbage();
  }
  if (fraction.length > digits) {
    return fail(
      digits === 0
        ? say(t, "price_lists.amount_no_decimals")
        : say(t, "price_lists.amount_too_many_decimals", { digits }),
    );
  }
  const amount = BigInt((whole || "0") + fraction.padEnd(digits, "0"));
  if (amount > BigInt(MAX_AMOUNT)) return fail(say(t, "price_lists.amount_too_large"));
  return ok(Number(amount));
}

/** "1,234,567": one to three digits, then groups of exactly three. */
function isGrouped(text: string, separator: string): boolean {
  const [first, ...rest] = text.split(separator);
  return (
    first !== undefined && /^\d{1,3}$/.test(first) && rest.every((group) => /^\d{3}$/.test(group))
  );
}

/**
 * The decimal text of an amount in minor units, without grouping or a currency: "12.50",
 * "1234" for a currency without decimals, "-0.05". What the grid's inputs and the CSV
 * files hold.
 */
export function formatAmount(amount: number, digits: number): string {
  const negative = amount < 0;
  const text = String(Math.abs(amount)).padStart(digits + 1, "0");
  const whole = digits === 0 ? text : text.slice(0, -digits);
  const fraction = digits === 0 ? "" : `.${text.slice(-digits)}`;
  return `${negative ? "-" : ""}${whole}${fraction}`;
}

/**
 * A read-only amount as a grid cell shows it: plain decimals when it's in the list's
 * currency (the column header names that), with its own code after when it isn't
 * ("12.50 USD": a variant's base price may be in another currency).
 */
export function cellAmount(
  money: { amount: number; currency: string },
  digits: number,
  listCurrency: string,
): string {
  const text = formatAmount(money.amount, digits);
  return money.currency === listCurrency ? text : `${text} ${money.currency}`;
}

/**
 * `amount` adjusted by `bps` basis points (−1000 is 10% off), rounded half away from
 * zero in minor units: the backend's `Money::adjust`, so the grid shows what the API
 * computes. Null when the result is out of range.
 */
export function adjustAmount(amount: number, bps: number): number | null {
  const scale = 10_000n;
  const numerator = BigInt(amount) * (scale + BigInt(bps));
  // BigInt division truncates towards zero and the remainder takes the dividend's sign,
  // like Rust's integers.
  let result = numerator / scale;
  const remainder = numerator % scale;
  const abs = remainder < 0n ? -remainder : remainder;
  if (abs * 2n >= scale) result += numerator < 0n ? -1n : 1n;
  if (result > BigInt(MAX_AMOUNT) || result < -BigInt(MAX_AMOUNT)) return null;
  return Number(result);
}

/**
 * Basis points from a typed percentage, negative for a discount: "-10", "+5.5", "10%",
 * "−12,25" (a typographic minus and a decimal comma too). At most two decimals, from
 * −100% to +10,000%. The error is in `t`'s language (English without one).
 */
export function parsePercent(input: string, t?: PriceT): Parsed<number> {
  const match = /^([-+−])?\s*(\d+)(?:[.,](\d+))?\s*%?$/.exec(input.trim());
  if (!match) return fail(say(t, "price_lists.percent_invalid"));
  const [, sign, whole = "", fraction = ""] = match;
  if (fraction.length > 2) return fail(say(t, "price_lists.percent_too_many_decimals"));
  const magnitude = Number(whole) * 100 + Number(fraction.padEnd(2, "0"));
  if (!Number.isSafeInteger(magnitude)) return fail(say(t, "price_lists.percent_too_large"));
  // `-0` would read back as "-0".
  const bps = (sign === "-" || sign === "−") && magnitude > 0 ? -magnitude : magnitude;
  if (bps < MIN_BASIS_POINTS) return fail(say(t, "price_lists.percent_below_minimum"));
  if (bps > MAX_BASIS_POINTS) return fail(say(t, "price_lists.percent_above_maximum"));
  return ok(bps);
}
