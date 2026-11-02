#!/usr/bin/env node
// Generates hypha-addresses' country tables from a checkout of Shopify's `worldwide`
// (https://github.com/Shopify/worldwide, MIT, with Unicode CLDR data):
//
//   node scripts/address-data.mjs <path to a worldwide checkout>
//
// It reads only YAML (`data/regions/*.yml`, and each country's `en.yml` for its labels),
// never the gem's code, and writes `crates/hypha-addresses/src/generated/` with the
// commit it read. Run it by hand to take an update; review the diff like code.
//
// The rules follow the gem's own (`Region#zip_required?`, `AddressValidator`):
// - a postal code is required when `zip_requirement` is `required`, or when it's absent
//   and the country has a `zip_regex`; `recommended` and `optional` aren't required;
//   countries whose single code is filled in (`zip_autofill_enabled`) don't show one;
// - the region is shown when the form (`format.edit`) has it, listed when the country
//   has zones, and required when it's listed and not `province_optional`.
// Postal code patterns are Ruby regexes, rewritten for Rust's `regex` crate: `\d` becomes
// `[0-9]`, a leading negative lookahead becomes a separate pattern the code must not
// match, and anything else the crate can't run stops the script.

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

const source = process.argv[2];
if (!source) {
  console.error("usage: node scripts/address-data.mjs <path to a worldwide checkout>");
  process.exit(2);
}
const regionsDir = join(source, "data", "regions");
const out = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "crates",
  "hypha-addresses",
  "src",
  "generated",
);

const commit = execFileSync("git", ["-C", source, "rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
const committed = execFileSync("git", ["-C", source, "log", "-1", "--format=%cs"], {
  encoding: "utf8",
}).trim();

// worldwide's form tokens → Hypha's address fields (`country` is the form's own select).
const FIELDS = {
  firstName: "FirstName",
  lastName: "LastName",
  company: "Company",
  address1: "Line1",
  address2: "Line2",
  city: "City",
  province: "Province",
  zip: "PostalCode",
  phone: "Phone",
};
// worldwide's label keys → the same fields.
const LABEL_KEYS = {
  first_name: "FirstName",
  last_name: "LastName",
  company: "Company",
  address1: "Line1",
  address2: "Line2",
  city: "City",
  province: "Province",
  zip: "PostalCode",
  phone: "Phone",
};

function fail(message) {
  console.error(`address-data: ${message}`);
  process.exit(1);
}

function readYaml(path) {
  return parse(readFileSync(path, "utf8"));
}

/** The English labels a country overrides, by Hypha field. */
function labelsOf(code) {
  const file = join(regionsDir, code, "en.yml");
  if (!existsSync(file)) return {};
  const addresses = readYaml(file)?.en?.worldwide?.[code]?.addresses ?? {};
  const labels = {};
  for (const [key, value] of Object.entries(addresses)) {
    const field = LABEL_KEYS[key];
    const label = value?.label?.default;
    if (field && typeof label === "string") labels[field] = label;
  }
  return labels;
}

/**
 * Ruby's `\d` is ASCII, the crate's every Unicode digit; and postal codes are compared
 * with their spaces collapsed, so the pattern's are too.
 */
function ascii(pattern) {
  return pattern.replaceAll("\\d", "[0-9]").replace(/ {2,}/g, " ");
}

/** A Ruby pattern rewritten for the `regex` crate: `{pattern, exclude}`. */
function rewrite(code, ruby) {
  let pattern = ruby;
  let exclude = null;
  // `before(?!X)after` → `before after`, refused when `before(?:X)` matches its start.
  const at = pattern.indexOf("(?!");
  if (at >= 0) {
    let depth = 0;
    let end = -1;
    for (let i = at; i < pattern.length; i++) {
      if (pattern[i] === "\\") i++;
      else if (pattern[i] === "(") depth++;
      else if (pattern[i] === ")" && --depth === 0) {
        end = i;
        break;
      }
    }
    if (end < 0) fail(`${code}: unbalanced lookahead in ${JSON.stringify(ruby)}`);
    const before = pattern.slice(0, at);
    if (!before.startsWith("^")) fail(`${code}: unanchored lookahead in ${JSON.stringify(ruby)}`);
    exclude = `${before}(?:${pattern.slice(at + 3, end)})`;
    pattern = before + pattern.slice(end + 1);
  }
  for (const construct of [
    "(?=",
    "(?!",
    "(?<=",
    "(?<!",
    "(?>",
    "\\G",
    "\\K",
    "\\h",
    "\\R",
    "\\X",
    "*+",
    "++",
    "?+",
  ]) {
    if (pattern.includes(construct))
      fail(`${code}: can't rewrite ${construct} in ${JSON.stringify(ruby)}`);
  }
  if (/\\[1-9]/.test(pattern)) fail(`${code}: backreference in ${JSON.stringify(ruby)}`);

  pattern = ascii(pattern);
  // Unanchored patterns are meant whole.
  if (!pattern.startsWith("^")) pattern = `^(?:${pattern}`;
  else pattern = `^(?:${pattern.slice(1)}`;
  pattern = pattern.endsWith("$") ? `${pattern.slice(0, -1)})$` : `${pattern})$`;
  return { pattern, exclude: exclude && ascii(exclude) };
}

/** What a postal code is compared on: upper case, single spaces. */
function normalizePostal(value) {
  return value.trim().toUpperCase().replace(/\s+/g, " ");
}

function rust(value) {
  return JSON.stringify(value).replace(/\\u([0-9a-f]{4})/g, "\\u{$1}");
}

const layouts = new Map();
const countries = [];

for (const file of readdirSync(regionsDir)
  .filter((f) => /^[A-Z]{2}\.yml$/.test(f))
  .toSorted()) {
  const spec = readYaml(join(regionsDir, file));
  const code = spec.code;
  if (code !== file.slice(0, 2)) fail(`${file}: code ${code}`);

  const edit = spec.format?.edit;
  if (typeof edit !== "string") fail(`${code}: no format.edit`);
  const rows = edit
    .split("_")
    .map((row) =>
      [...row.matchAll(/\{([A-Za-z0-9]+)\}/g)]
        .map((m) => m[1])
        .filter((token) => token !== "country")
        .map((token) => FIELDS[token] ?? fail(`${code}: unknown form token {${token}}`)),
    )
    .filter((row) => row.length > 0);
  const shown = new Set(rows.flat());

  // Countries with one code filled in (`zip_autofill_enabled`) don't show it.
  if (spec.zip_autofill_enabled === true && shown.has("PostalCode"))
    fail(`${code}: a filled postal code in the form`);
  const requirement = spec.zip_requirement;
  let postal;
  if (!shown.has("PostalCode")) postal = "None";
  else if (requirement === "required" || (requirement == null && spec.zip_regex != null))
    postal = "Required";
  else if (requirement === "recommended") postal = "Recommended";
  else postal = "Optional";

  let pattern = null;
  if (spec.zip_regex != null && postal !== "None") {
    pattern = rewrite(code, spec.zip_regex);
    const example = spec.zip_example == null ? null : normalizePostal(String(spec.zip_example));
    if (example) {
      const ok =
        new RegExp(pattern.pattern, "u").test(example) &&
        !(pattern.exclude && new RegExp(pattern.exclude, "u").test(example));
      if (!ok) fail(`${code}: example ${example} doesn't match ${pattern.pattern}`);
    }
  }

  const regionShown = shown.has("Province");
  const zones = regionShown ? (spec.zones ?? []).filter((z) => !z.deprecated) : [];
  const regions = zones.map((zone) => {
    if (!zone.code || !zone.name) fail(`${code}: zone without code or name`);
    const alternates = [...(zone.name_alternates ?? []), ...(zone.code_alternates ?? [])].map(
      String,
    );
    for (const name of [zone.code, zone.name, ...alternates]) {
      if (name.includes("|")) fail(`${code}: "|" in ${name}`);
    }
    const prefixes = (zone.zip_prefixes ?? []).map((p) =>
      normalizePostal(String(p)).replaceAll(" ", ""),
    );
    return { code: String(zone.code), name: String(zone.name), alternates, prefixes };
  });
  const regionRequired = regions.length > 0 && spec.province_optional !== true;

  const labels = labelsOf(code);
  if (spec.zip_label && !labels.PostalCode) labels.PostalCode = spec.zip_label;

  const layoutKey = JSON.stringify(rows);
  if (!layouts.has(layoutKey)) layouts.set(layoutKey, { name: `LAYOUT_${layouts.size}`, rows });

  countries.push({
    code,
    layout: layouts.get(layoutKey).name,
    postal,
    pattern,
    example: spec.zip_example == null ? null : String(spec.zip_example),
    regions,
    regionRequired,
    houseNumber: spec.building_number_required === true,
    labels,
  });
}

const header = `// @generated by scripts/address-data.mjs from Shopify/worldwide
// (https://github.com/Shopify/worldwide) at ${commit} (${committed}).
// Do not edit: run the script again. The data's licence is LICENSE.md beside this file.
`;

let body = `${header}
use crate::data::{Country, Field, PostalCode, Region};

`;
for (const { name, rows } of layouts.values()) {
  body += `static ${name}: &[&[Field]] = &[${rows.map((row) => `&[${row.map((f) => `Field::${f}`).join(", ")}]`).join(", ")}];\n`;
}
body += `\npub(crate) const COUNT: usize = ${countries.length};\n\n/// Every country worldwide lists, sorted by code.\npub(crate) static COUNTRIES: [Country; COUNT] = [\n`;
for (const c of countries) {
  const labels = Object.entries(c.labels)
    .toSorted(([a], [b]) => a.localeCompare(b))
    .map(([field, label]) => `(Field::${field}, ${rust(label)})`);
  body += `    Country {
        code: ${rust(c.code)},
        rows: ${c.layout},
        postal_code: PostalCode::${c.postal},
        postal_pattern: ${c.pattern ? `Some(${rust(c.pattern.pattern)})` : "None"},
        postal_exclude: ${c.pattern?.exclude ? `Some(${rust(c.pattern.exclude)})` : "None"},
        postal_example: ${c.example ? `Some(${rust(c.example)})` : "None"},
        regions: &[${c.regions
          .map(
            (r) =>
              `\n            Region { code: ${rust(r.code)}, name: ${rust(r.name)}, alternates: ${rust(r.alternates.join("|"))}, prefixes: ${rust(r.prefixes.join(" "))} },`,
          )
          .join("")}${c.regions.length ? "\n        " : ""}],
        region_required: ${c.regionRequired},
        house_number_required: ${c.houseNumber},
        labels: &[${labels.join(", ")}],
    },
`;
}
body += "];\n";

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "countries.rs"), body);
execFileSync("rustfmt", ["--edition", "2024", join(out, "countries.rs")]);
writeFileSync(join(out, "LICENSE.md"), readFileSync(join(source, "LICENSE.md"), "utf8"));
console.log(
  `address-data: ${countries.length} countries, ${layouts.size} layouts, ${countries.reduce((n, c) => n + c.regions.length, 0)} regions from ${commit}`,
);
