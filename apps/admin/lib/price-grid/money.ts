// Money as the grid and the CSV files type it: decimal text in and out, integer minor
// units in between. Nothing here goes through a float, so "0.29" is 29 cents, never
// 28.999….

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

/** How `currency` is written in English ("€", "$", "CA$"), longest first. */
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
  return [...symbols].sort((a, b) => b.length - a.length);
}

/**
 * `text` without `currency`'s code or symbol at its ends. Another currency's code or any
 * other currency sign is an error: "$12" in a EUR list is a mistake, not 12 euros.
 */
function stripCurrency(text: string, currency: string): Parsed<string> {
  const symbols = symbolsOf(currency);
  const foreign = (written: string) =>
    fail<string>(`That's in ${written}; this list is in ${currency}.`);
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
 * at either end; another currency's is refused.
 */
export function parseAmount(input: string, digits: number, currency: string): Parsed<number> {
  const stripped = stripCurrency(input.trim(), currency);
  if (!stripped.ok) return stripped;
  const text = stripped.value.replaceAll(GROUPING, "");
  if (text === "") return input.trim() === "" ? fail("Enter an amount.") : garbage(input);
  if (/^[-−]/.test(text)) return fail("Can't be negative.");
  if (!/^\+?[\d.,]+$/.test(text) || !/\d/.test(text)) return garbage(input);
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
    if (head.includes(body.charAt(point)) || !isGrouped(head, group)) return garbage(input);
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
        `${JSON.stringify(input.trim())} is ambiguous: write it without the separator, or with ${digits} decimals.`,
      );
    }
    if (parts.length > 2) {
      if (!isGrouped(body, separator)) return garbage(input);
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
    return garbage(input);
  }
  if (fraction.length > digits) {
    return fail(
      digits === 0 ? "This currency has no decimals." : `At most ${digits} decimal places.`,
    );
  }
  const amount = BigInt((whole || "0") + fraction.padEnd(digits, "0"));
  if (amount > BigInt(MAX_AMOUNT)) return fail("Too large.");
  return ok(Number(amount));
}

/** "1,234,567": one to three digits, then groups of exactly three. */
function isGrouped(text: string, separator: string): boolean {
  const [first, ...rest] = text.split(separator);
  return (
    first !== undefined && /^\d{1,3}$/.test(first) && rest.every((group) => /^\d{3}$/.test(group))
  );
}

function garbage<T>(input: string): Parsed<T> {
  return fail(`${JSON.stringify(input.trim())} isn't an amount.`);
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
 * −100% to +10,000%.
 */
export function parsePercent(input: string): Parsed<number> {
  const match = /^([-+−])?\s*(\d+)(?:[.,](\d+))?\s*%?$/.exec(input.trim());
  if (!match) return fail("A percentage, like -10 or 5.5.");
  const [, sign, whole = "", fraction = ""] = match;
  if (fraction.length > 2) return fail("At most two decimal places.");
  const magnitude = Number(whole) * 100 + Number(fraction.padEnd(2, "0"));
  if (!Number.isSafeInteger(magnitude)) return fail("Too large.");
  // `-0` would read back as "-0".
  const bps = (sign === "-" || sign === "−") && magnitude > 0 ? -magnitude : magnitude;
  if (bps < MIN_BASIS_POINTS) return fail("Can't take off more than 100%.");
  if (bps > MAX_BASIS_POINTS) return fail("At most +10,000%.");
  return ok(bps);
}
