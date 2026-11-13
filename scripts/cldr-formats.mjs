#!/usr/bin/env node
// Generates hypha-mail's table of number, currency and date formats per language from
// Unicode CLDR's JSON packages (`cldr-numbers-full` and `cldr-dates-full` on npm):
//
//   mkdir /tmp/cldr && cd /tmp/cldr
//   npm pack cldr-numbers-full@48.2.0 cldr-dates-full@48.2.0
//   mkdir numbers dates
//   tar -xzf cldr-numbers-full-48.2.0.tgz -C numbers
//   tar -xzf cldr-dates-full-48.2.0.tgz -C dates
//   node scripts/cldr-formats.mjs /tmp/cldr/numbers/package /tmp/cldr/dates/package
//
// It reads only the packages' JSON and writes `crates/hypha-mail/src/generated/formats.rs`.
// Run it by hand to take an update or add a language (`LANGUAGES` below); review the diff
// like code. The packages aren't a dependency of the workspace.
//
// Per language: the decimal and group marks, the minus sign, CLDR's minimum grouping digits,
// where the currency sign goes (the standard currency pattern), the symbols of a few
// currencies, abbreviated and full month names (the format forms: the case a day's date
// takes), and two patterns: the `yMMMd` date and that date with the `Hm` time joined by the
// medium date-time pattern. Patterns are tokenized here, so the Rust side only fills them.
//
// English (`en`) keeps what emails wrote before languages: the day before the month
// ("18 Oct 2026", en-GB's order), symbols for the euro, dollar and pound only, and other
// currencies by their code after the amount ("1,234.00 RSD").

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const [numbersDir, datesDir] = process.argv.slice(2);
if (!numbersDir || !datesDir) {
  console.error(
    "usage: node scripts/cldr-formats.mjs <cldr-numbers-full package> <cldr-dates-full package>",
  );
  process.exit(2);
}

/** The languages the build formats for, and the CLDR locale each reads (pt is Brazil's). */
const LANGUAGES = [
  ["en", "en"],
  ["en-GB", "en-GB"],
  ["de", "de"],
  ["de-AT", "de-AT"],
  ["de-CH", "de-CH"],
  ["fr", "fr"],
  ["it", "it"],
  ["es", "es"],
  ["nl", "nl"],
  ["pt", "pt-PT"],
  ["pt-BR", "pt"],
  ["sr-Latn", "sr-Latn"],
  ["sr", "sr"],
  ["hr", "hr"],
  ["bs", "bs"],
  ["sl", "sl"],
  ["pl", "pl"],
  ["cs", "cs"],
  ["sv", "sv"],
  ["da", "da"],
  ["nb", "nb"],
  ["fi", "fi"],
];

/** Currencies whose symbol every language carries (when CLDR gives it one). */
const COMMON = ["EUR", "USD", "GBP"];
/** Each language's own currency, when it isn't a common one. */
const LOCAL = {
  "de-CH": "CHF",
  pl: "PLN",
  cs: "CZK",
  sv: "SEK",
  da: "DKK",
  nb: "NOK",
  sr: "RSD",
  "sr-Latn": "RSD",
  bs: "BAM",
  "pt-BR": "BRL",
};

const read = (path) => JSON.parse(readFileSync(path, "utf8"));
const version = read(join(numbersDir, "package.json")).version;
const datesVersion = read(join(datesDir, "package.json")).version;
if (version !== datesVersion) {
  console.error(`cldr-numbers-full ${version} and cldr-dates-full ${datesVersion} differ`);
  process.exit(1);
}

/** A CLDR date pattern as parts: `d. MMM y` → Day, ".", " ", MonthShort, " ", Year. */
function tokenize(pattern, locale) {
  const parts = [];
  const literal = (text) => {
    const last = parts.at(-1);
    if (last && last.lit !== undefined) last.lit += text;
    else parts.push({ lit: text });
  };
  const fields = {
    d: "Day",
    dd: "Day2",
    M: "MonthNumber",
    MM: "MonthNumber2",
    L: "MonthNumber",
    LL: "MonthNumber2",
    MMM: "MonthShort",
    LLL: "MonthShort",
    MMMM: "MonthLong",
    LLLL: "MonthLong",
    y: "Year",
    yyyy: "Year",
    H: "Hour",
    HH: "Hour2",
    m: "Minute2",
    mm: "Minute2",
  };
  let i = 0;
  while (i < pattern.length) {
    const c = pattern[i];
    if (c === "'") {
      // '' is an apostrophe; 'text' is literal text.
      if (pattern[i + 1] === "'") {
        literal("'");
        i += 2;
        continue;
      }
      const end = pattern.indexOf("'", i + 1);
      if (end < 0) throw new Error(`${locale}: unclosed quote in ${JSON.stringify(pattern)}`);
      literal(pattern.slice(i + 1, end));
      i = end + 1;
      continue;
    }
    if (/[A-Za-z]/.test(c)) {
      let j = i;
      while (pattern[j] === c) j++;
      const token = pattern.slice(i, j);
      const field = fields[token];
      if (!field)
        throw new Error(`${locale}: unsupported field ${token} in ${JSON.stringify(pattern)}`);
      parts.push({ field });
      i = j;
      continue;
    }
    literal(c);
    i++;
  }
  return parts;
}

/** `{1}, {0}` with the date's and the time's parts in. */
function joinDateTime(glue, date, time, locale) {
  const parts = [];
  for (const piece of glue.split(/(\{[01]\})/)) {
    if (piece === "{1}") parts.push(...date);
    else if (piece === "{0}") parts.push(...time);
    else if (piece !== "") {
      if (piece.includes("'")) throw new Error(`${locale}: quoted text in ${JSON.stringify(glue)}`);
      parts.push({ lit: piece });
    }
  }
  // Join neighbouring literals.
  return parts.reduce((out, part) => {
    const last = out.at(-1);
    if (part.lit !== undefined && last && last.lit !== undefined) last.lit += part.lit;
    else out.push({ ...part });
    return out;
  }, []);
}

// Spaces other than U+0020 (no-break, narrow no-break) are written as escapes, so the
// table shows them.
const rust = (text) =>
  JSON.stringify(text).replace(/[^\S ]/gu, (c) => `\\u{${c.codePointAt(0).toString(16)}}`);
const partsRust = (parts) =>
  `&[${parts.map((p) => (p.lit !== undefined ? `Part::Lit(${rust(p.lit)})` : `Part::${p.field}`)).join(", ")}]`;

const entries = [];
for (const [tag, locale] of LANGUAGES) {
  const numbers = read(join(numbersDir, "main", locale, "numbers.json")).main[locale].numbers;
  const currencies = read(join(numbersDir, "main", locale, "currencies.json")).main[locale].numbers
    .currencies;
  const gregorian = read(join(datesDir, "main", locale, "ca-gregorian.json")).main[locale].dates
    .calendars.gregorian;
  if (numbers.defaultNumberingSystem !== "latn") {
    throw new Error(`${locale}: numbering system ${numbers.defaultNumberingSystem}, not latn`);
  }
  const symbols = numbers["symbols-numberSystem-latn"];
  const standard = numbers["currencyFormats-numberSystem-latn"].standard.split(";")[0];
  const sign = standard.indexOf("¤");
  const number = standard.search(/[#0]/);
  if (sign < 0 || number < 0) throw new Error(`${locale}: currency pattern ${standard}`);
  const before = sign < number;
  const between = before
    ? standard.slice(sign + 1, number)
    : standard.slice(standard.lastIndexOf("0") + 1, sign);
  if (!/^[  ]?$/.test(between)) {
    throw new Error(`${locale}: unexpected text between sign and number in ${standard}`);
  }
  const english = tag === "en";
  const wanted = english ? COMMON : [...COMMON, ...(LOCAL[tag] ? [LOCAL[tag]] : [])];
  const symbolList = wanted
    .map((code) => [code, currencies[code]?.symbol])
    .filter(([code, symbol]) => symbol && symbol !== code);
  const months = gregorian.months.format;
  const short = Array.from({ length: 12 }, (_, i) => months.abbreviated[String(i + 1)]);
  const long = Array.from({ length: 12 }, (_, i) => months.wide[String(i + 1)]);
  const available = gregorian.dateTimeFormats.availableFormats;
  // English keeps the day first, as before (en-GB's order; en's own is "Oct 18, 2026").
  const datePattern = english
    ? read(join(datesDir, "main", "en-GB", "ca-gregorian.json")).main["en-GB"].dates.calendars
        .gregorian.dateTimeFormats.availableFormats.yMMMd
    : available.yMMMd;
  const date = tokenize(datePattern, locale);
  const time = tokenize(available.Hm, locale);
  const glue = gregorian.dateTimeFormats.medium;
  if (typeof glue !== "string") throw new Error(`${locale}: medium date-time isn't a pattern`);
  const dateTime = joinDateTime(glue, date, time, locale);
  entries.push(`    Formats {
        tag: ${rust(tag)},
        decimal: ${rust(symbols.decimal)},
        group: ${rust(symbols.group)},
        minus: ${rust(symbols.minusSign)},
        min_grouping: ${Number(numbers.minimumGroupingDigits)},
        sign_before: ${before},
        sign_space: ${rust(between)},
        codes_after: ${english},
        symbols: &[${symbolList.map(([c, s]) => `(${rust(c)}, ${rust(s)})`).join(", ")}],
        months: [${short.map(rust).join(", ")}],
        months_long: [${long.map(rust).join(", ")}],
        date: ${partsRust(date)},
        date_time: ${partsRust(dateTime)},
    },`);
}

const out = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "crates",
  "hypha-mail",
  "src",
  "generated",
);
mkdirSync(out, { recursive: true });
const file = join(out, "formats.rs");
writeFileSync(
  file,
  `// @generated by scripts/cldr-formats.mjs from Unicode CLDR ${version} (cldr-numbers-full,
// cldr-dates-full; Unicode License v3). Do not edit: run the script again.

use crate::format::{Formats, Part};

/// Every language the build formats for, English first (the fallback).
pub(crate) static FORMATS: &[Formats] = &[
${entries.join("\n")}
];
`,
);
execFileSync("rustfmt", ["--edition", "2024", file]);
console.log(`wrote ${file} (${LANGUAGES.length} languages, CLDR ${version})`);
