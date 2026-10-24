// A price list's prices as a CSV file, out and back in. The import is previewed before
// anything is written: each row matched to a variant (by variant ID, else SKU, else
// handle and options) and checked by the grid's own rules, and the result is the grid's
// diff, sent the same way. Plain string work, no browser or Node APIs, so it runs in a
// server action on any host.

import { formatBreaks } from "./price-grid/breaks.ts";
import { checkDraft, sameFixed } from "./price-grid/edits.ts";
import { formatAmount } from "./price-grid/money.ts";
import type {
  FixedPriceUpsert,
  Money,
  PriceGridDiff,
  PriceGridProduct,
  PriceGridVariant,
} from "./price-grid/types.ts";

/** Minor-unit digits by currency code, as `lib/format.ts`'s `Currencies`. */
type Currencies = Readonly<Record<string, number>>;

/** The most fixed prices one `PUT …/fixed-prices` takes. */
export const MAX_PRICES_PER_REQUEST = 1000;

const BASE_COLUMNS = [
  "handle",
  "sku",
  "variant_id",
  "options",
  "base_price",
  "fixed_price",
  "compare_at",
] as const;

// ---- writing ----

// A spreadsheet runs a cell starting with one of these as a formula; a quote in front
// makes it text (and the import takes it off again).
const FORMULA_START = /^[=+\-@\t\r]/;

function textCell(value: string): string {
  return FORMULA_START.test(value) ? `'${value}` : value;
}

function quoteCsv(cell: string, delimiter: string): string {
  return cell.includes(delimiter) || /["\r\n]/.test(cell)
    ? `"${cell.replaceAll('"', '""')}"`
    : cell;
}

/** RFC 4180 text: CRLF line ends, fields quoted when they hold the delimiter, a quote or a line break. */
export function toCsv(rows: readonly (readonly string[])[], delimiter = ","): string {
  return (
    rows.map((row) => row.map((cell) => quoteCsv(cell, delimiter)).join(delimiter)).join("\r\n") +
    "\r\n"
  );
}

function amount(money: Money | null, currencies: Currencies): string {
  return money ? formatAmount(money.amount, currencies[money.currency] ?? 2) : "";
}

/**
 * The list's prices, one row per variant: handle, SKU, variant ID, options ("M / Blue",
 * empty without options), the base price (in the variant's own currency), the fixed
 * price and its compare-at, then `break_<n>_qty` / `break_<n>_price` for as many breaks
 * as the most any variant has. Amounts are plain decimals ("12.50"); empty is none.
 */
export function exportPriceCsv(
  products: readonly PriceGridProduct[],
  currencies: Currencies,
): string {
  const breakColumns = Math.max(
    0,
    ...products.flatMap((p) => p.variants.map((v) => v.fixed?.breaks.length ?? 0)),
  );
  const header: string[] = [...BASE_COLUMNS];
  for (let n = 1; n <= breakColumns; n++) header.push(`break_${n}_qty`, `break_${n}_price`);

  const rows = products.flatMap((product) =>
    product.variants.map((variant) => {
      const fixed = variant.fixed;
      const row = [
        textCell(product.handle),
        textCell(variant.sku ?? ""),
        variant.variant,
        textCell(variant.title ?? ""),
        amount(variant.base_price, currencies),
        amount(fixed?.price ?? null, currencies),
        amount(fixed?.compare_at ?? null, currencies),
      ];
      for (let n = 0; n < breakColumns; n++) {
        const b = fixed?.breaks[n];
        row.push(b ? String(b.min_quantity) : "", amount(b?.price ?? null, currencies));
      }
      return row;
    }),
  );
  return toCsv([header, ...rows]);
}

// ---- reading ----

/**
 * The delimiter a file uses: a comma, a semicolon (spreadsheets where the decimal point
 * is a comma) or a tab, whichever its first line has most of outside quotes.
 */
export function detectDelimiter(text: string): "," | ";" | "\t" {
  const counts = { ",": 0, ";": 0, "\t": 0 };
  let quoted = false;
  for (const char of text) {
    if (char === '"') quoted = !quoted;
    else if (!quoted && (char === "\n" || char === "\r")) break;
    else if (!quoted && (char === "," || char === ";" || char === "\t")) counts[char]++;
  }
  if (counts[";"] > counts[","] && counts[";"] >= counts["\t"]) return ";";
  if (counts["\t"] > counts[","]) return "\t";
  return ",";
}

/**
 * Rows of fields from CSV text (RFC 4180): a leading BOM dropped, CRLF or LF line ends,
 * quoted fields holding delimiters, line breaks and doubled quotes. Blank lines are
 * skipped. The delimiter is detected unless given.
 */
export function parseCsv(
  input: string,
  delimiter = detectDelimiter(input.replace(/^﻿/, "")),
): string[][] {
  const text = input.replace(/^﻿/, "");
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let atFieldStart = true;

  const endRow = () => {
    row.push(field);
    if (!(row.length === 1 && row[0] === "")) rows.push(row);
    row = [];
    field = "";
    atFieldStart = true;
  };

  for (let i = 0; i < text.length; i++) {
    const char = text.charAt(i);
    if (quoted) {
      if (char === '"' && text.charAt(i + 1) === '"') {
        field += '"';
        i++;
      } else if (char === '"') {
        quoted = false;
      } else {
        field += char;
      }
    } else if (char === '"' && atFieldStart) {
      quoted = true;
      atFieldStart = false;
    } else if (char === delimiter) {
      row.push(field);
      field = "";
      atFieldStart = true;
    } else if (char === "\r" || char === "\n") {
      if (char === "\r" && text.charAt(i + 1) === "\n") i++;
      endRow();
    } else {
      field += char;
      atFieldStart = false;
    }
  }
  if (field !== "" || row.length > 0) endRow();
  return rows;
}

export type ImportStatus = "changed" | "unchanged" | "unmatched" | "invalid";

export type ImportRow = {
  /** The row's line in the file, the header being line 1. */
  line: number;
  status: ImportStatus;
  /** The matched variant, when there is one. */
  variant?: string;
  /** What the row does ("Sets the fixed price", "Removes the fixed price") or why it can't. */
  reasons: string[];
};

export type ImportPreview = {
  /** Every data row, in the file's order. */
  rows: ImportRow[];
  counts: Record<ImportStatus, number>;
  /** What applying writes: the grid's diff. */
  diff: PriceGridDiff;
  /** Why the file can't be read at all (no header, no identifying column); then no rows. */
  error?: string;
};

/** A row's fields by column name, matched ignoring case; a missing column reads empty. */
function fieldReader(header: readonly string[]) {
  const index = new Map(header.map((name, i) => [name.trim().toLowerCase(), i]));
  return {
    has: (name: string) => index.has(name),
    get: (row: readonly string[], name: string): string => {
      const i = index.get(name);
      const value = i === undefined ? "" : (row[i] ?? "").trim();
      // The quote an export puts before a formula-like text.
      return /^'[=+\-@\t\r]/.test(value) ? value.slice(1) : value;
    },
  };
}

const sameText = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/**
 * What importing `text` into a list in `currency` would do, against the prices the
 * list has now (`products`, the whole list or the part being replaced). Nothing is
 * written: the preview's `diff` is what applying sends.
 */
export function previewPriceImport(
  text: string,
  products: readonly PriceGridProduct[],
  currency: string,
  currencies: Currencies,
): ImportPreview {
  const counts: Record<ImportStatus, number> = {
    changed: 0,
    unchanged: 0,
    unmatched: 0,
    invalid: 0,
  };
  const diff: PriceGridDiff = { upserts: [], clears: [] };
  const fail = (error: string): ImportPreview => ({ rows: [], counts, diff, error });

  const digits = currencies[currency];
  if (digits === undefined) return fail(`Unknown currency ${JSON.stringify(currency)}.`);
  const [header, ...data] = parseCsv(text);
  if (!header) return fail("The file is empty.");
  const field = fieldReader(header);
  if (!field.has("fixed_price")) return fail("The file has no fixed_price column.");
  if (!field.has("variant_id") && !field.has("sku") && !field.has("handle")) {
    return fail("The file needs a variant_id, sku or handle column to find each variant.");
  }
  // The break numbers the header names, in order: a file may skip some (break_1, break_3).
  const breakNumbers = [
    ...new Set(
      header.flatMap((name) => {
        const match = /^break_(\d+)_(?:qty|price)$/i.exec(name.trim());
        return match ? [Number(match[1])] : [];
      }),
    ),
  ].sort((a, b) => a - b);

  const byId = new Map<string, PriceGridVariant>();
  const bySku = new Map<string, PriceGridVariant[]>();
  const byHandle = new Map<string, PriceGridProduct>();
  for (const product of products) {
    byHandle.set(product.handle.toLowerCase(), product);
    for (const variant of product.variants) {
      byId.set(variant.variant, variant);
      if (variant.sku) {
        const key = variant.sku.toLowerCase();
        bySku.set(key, [...(bySku.get(key) ?? []), variant]);
      }
    }
  }
  const seen = new Map<string, number>();

  const rows = data.map((row, index): ImportRow => {
    const line = index + 2;
    const done = (status: ImportStatus, reasons: string[], variant?: string): ImportRow => {
      counts[status]++;
      return { line, status, variant, reasons };
    };

    // Find the variant.
    const id = field.get(row, "variant_id");
    const sku = field.get(row, "sku");
    const handle = field.get(row, "handle");
    let variant: PriceGridVariant | undefined;
    if (id) {
      variant = byId.get(id);
      if (!variant) return done("unmatched", [`No variant ${id} in this list.`]);
    } else if (sku) {
      const found = bySku.get(sku.toLowerCase()) ?? [];
      if (found.length > 1)
        return done("invalid", [`SKU ${sku} is on ${found.length} variants: add variant_id.`]);
      variant = found[0];
      if (!variant) return done("unmatched", [`No variant with SKU ${sku}.`]);
    } else if (handle) {
      const product = byHandle.get(handle.toLowerCase());
      if (!product) return done("unmatched", [`No product with handle ${handle}.`]);
      const options = field.get(row, "options");
      variant = product.variants.find((v) => sameText(v.title ?? "", options));
      if (!variant) {
        return done("unmatched", [
          options
            ? `${handle} has no variant ${options}.`
            : `${handle} has options: fill in the options column.`,
        ]);
      }
    } else {
      return done("invalid", ["No variant_id, SKU or handle to find the variant by."]);
    }
    const earlier = seen.get(variant.variant);
    if (earlier !== undefined) {
      return done("invalid", [`The same variant as line ${earlier}.`], variant.variant);
    }
    seen.set(variant.variant, line);

    // Read its prices.
    const reasons: string[] = [];
    const breaks: string[] = [];
    for (const n of breakNumbers) {
      const qty = field.get(row, `break_${n}_qty`);
      const price = field.get(row, `break_${n}_price`);
      if (qty === "" && price === "") continue;
      if (qty === "" || price === "") reasons.push(`Break ${n} needs both a quantity and a price.`);
      else if (!/^\d+$/.test(qty))
        reasons.push(`break_${n}_qty: ${JSON.stringify(qty)} isn't a whole number.`);
      else breaks.push(`${qty}=${price}`);
    }
    if (reasons.length > 0) return done("invalid", reasons, variant.variant);

    // A column the file doesn't have keeps what the variant has, so a `sku,fixed_price`
    // file changes prices without wiping compare-ats and breaks. An empty fixed price
    // still removes the whole fixed price.
    const fixedPrice = field.get(row, "fixed_price");
    const stored = fixedPrice === "" ? null : variant.fixed;
    const compareAt = field.has("compare_at")
      ? field.get(row, "compare_at")
      : stored?.compare_at
        ? formatAmount(stored.compare_at.amount, digits)
        : "";
    const breaksText =
      breakNumbers.length > 0
        ? breaks.join("; ")
        : formatBreaks(
            (stored?.breaks ?? []).map((b) => ({
              min_quantity: b.min_quantity,
              amount: b.price.amount,
            })),
            digits,
          );
    const checked = checkDraft(
      { fixed_price: fixedPrice, fixed_compare_at: compareAt, breaks: breaksText },
      currency,
      digits,
    );
    if (checked.kind === "invalid") {
      const { fixed_price, fixed_compare_at, breaks: breakError } = checked.errors;
      return done(
        "invalid",
        [
          fixed_price && `fixed_price: ${fixed_price}`,
          fixed_compare_at && `compare_at: ${fixed_compare_at}`,
          breakError && `Breaks: ${breakError}`,
        ].filter((reason): reason is string => Boolean(reason)),
        variant.variant,
      );
    }
    if (checked.kind === "none") {
      if (!variant.fixed) return done("unchanged", [], variant.variant);
      diff.clears.push({ variant: variant.variant });
      return done("changed", ["Removes the fixed price."], variant.variant);
    }
    if (variant.fixed && sameFixed(checked.fixed, variant.fixed)) {
      return done("unchanged", [], variant.variant);
    }
    diff.upserts.push({ variant: variant.variant, ...checked.fixed });
    return done(
      "changed",
      [variant.fixed ? "Changes the fixed price." : "Sets a fixed price."],
      variant.variant,
    );
  });

  return { rows, counts, diff };
}

/**
 * The variant IDs a file names, when every data row names one: then only those variants
 * need reading. Null when some row finds its variant another way (by SKU, or handle and
 * options), which takes the whole catalog.
 */
export function importVariantIds(text: string): string[] | null {
  const [header, ...data] = parseCsv(text);
  if (!header) return [];
  const field = fieldReader(header);
  if (!field.has("variant_id")) return null;
  const ids = new Set<string>();
  for (const row of data) {
    const id = field.get(row, "variant_id");
    if (id === "") return null;
    ids.add(id);
  }
  return [...ids];
}

/** `items` in runs of at most `size`. */
export function chunk<T>(items: readonly T[], size = MAX_PRICES_PER_REQUEST): T[][] {
  const chunks: T[][] = [];
  for (let i = 0; i < items.length; i += size) chunks.push(items.slice(i, i + size));
  return chunks;
}

/**
 * The requests a diff takes: each `PUT …/fixed-prices` body's entries and each
 * `DELETE …/fixed-prices` body's variants, at most 1,000 apiece.
 */
export function diffRequests(
  diff: PriceGridDiff,
  size = MAX_PRICES_PER_REQUEST,
): { upserts: FixedPriceUpsert[][]; clears: string[][] } {
  return {
    upserts: chunk(diff.upserts, size),
    clears: chunk(
      diff.clears.map((c) => c.variant),
      size,
    ),
  };
}
