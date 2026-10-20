// Custom fields (design note 0033) as the admin shows and edits them: owner types, field
// types and their validations, an input's state for each type and the JSON value it
// stands for, and the `field[…]` list filters. Plain data and pure functions, shared by
// server pages and client components.

import type { AdminSchemas } from "@hypha/sdk/admin";

import { type Currencies, moneyInput, parseMoney } from "./format";
import { hasScope } from "./navigation";

export type Definition = AdminSchemas["CustomFieldDefinitionResponse"];
export type Validations = AdminSchemas["CustomFieldValidations"];
export type FieldsResponse = AdminSchemas["CustomFieldsResponse"];

/**
 * A free-form JSON value where the SDK expects one of its "any object" schemas: the
 * generator writes those (`value_type = Object`) as `Record<string, never>`, though the
 * API takes any JSON there (a field's values, a bound of its validations).
 */
export function jsonObject(value: unknown): Record<string, never> {
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the schema says "any JSON"; only its generated TypeScript is narrower
  return value as Record<string, never>;
}

/** The records fields belong to (§2). */
export type OwnerType = "product" | "variant" | "category" | "customer" | "order";

export const OWNER_TYPES: {
  value: OwnerType;
  label: string;
  /** The owner's read and write scopes: values follow them (§5.1). */
  read: string;
  write: string;
}[] = [
  { value: "product", label: "Products", read: "products:read", write: "products:write" },
  { value: "variant", label: "Variants", read: "products:read", write: "products:write" },
  { value: "category", label: "Categories", read: "products:read", write: "products:write" },
  { value: "customer", label: "Customers", read: "customers:read", write: "customers:write" },
  { value: "order", label: "Orders", read: "orders:read", write: "orders:write" },
];

export function isOwnerType(value: string | undefined): value is OwnerType {
  return OWNER_TYPES.some((o) => o.value === value);
}

export function ownerOf(type: OwnerType) {
  return OWNER_TYPES.find((o) => o.value === type) ?? OWNER_TYPES[0];
}

/** Only customers' and orders' fields can be about a person (§5.2). */
export function allowsPersonal(type: OwnerType): boolean {
  return type === "customer" || type === "order";
}

export type BaseType =
  | "text"
  | "multiline_text"
  | "rich_text"
  | "integer"
  | "decimal"
  | "boolean"
  | "date"
  | "date_time"
  | "url"
  | "money"
  | "measurement"
  | "choice"
  | "reference"
  | "json";

export const BASE_TYPES: { value: BaseType; label: string; description: string }[] = [
  { value: "text", label: "Single line text", description: "Up to 255 characters." },
  { value: "multiline_text", label: "Multi-line text", description: "Plain text, up to 16 KB." },
  { value: "rich_text", label: "Rich text", description: "Formatted text (HTML)." },
  { value: "integer", label: "Integer", description: "A whole number." },
  { value: "decimal", label: "Decimal", description: "An exact number, like 12.50." },
  { value: "boolean", label: "True or false", description: "A switch." },
  { value: "date", label: "Date", description: "A day, like 2026-10-20." },
  { value: "date_time", label: "Date and time", description: "A moment, stored in UTC." },
  { value: "url", label: "URL", description: "A web, email or phone link." },
  { value: "money", label: "Money", description: "An amount in a currency." },
  { value: "measurement", label: "Measurement", description: "A weight, length or volume." },
  { value: "choice", label: "Choice", description: "One of a fixed set of choices." },
  { value: "reference", label: "Reference", description: "A product, category, customer…" },
  { value: "json", label: "JSON", description: "Any JSON, for integrations." },
];

/** Types that can't be listed (§3). */
const NOT_LISTABLE = new Set<BaseType>(["boolean", "rich_text", "multiline_text", "json"]);

export function isListable(base: BaseType): boolean {
  return !NOT_LISTABLE.has(base);
}

/** `list.text` → `{base: "text", isList: true}`. */
export function parseType(type: string): { base: BaseType; isList: boolean } {
  const isList = type.startsWith("list.");
  const name = isList ? type.slice(5) : type;
  const base = BASE_TYPES.find((t) => t.value === name)?.value ?? "json";
  return { base, isList };
}

/** "Single line text", "List of choices"… */
export function typeLabel(type: string): string {
  const { base, isList } = parseType(type);
  const label = BASE_TYPES.find((t) => t.value === base)?.label ?? type;
  return isList ? `List: ${label.toLowerCase()}` : label;
}

/** Which validations a type takes (`types.rs`' `allows`), lists adding their own. */
export function validationsOf(type: string): (keyof Validations)[] {
  const { base, isList } = parseType(type);
  const own: Record<BaseType, (keyof Validations)[]> = {
    text: ["min_length", "max_length", "pattern"],
    multiline_text: ["min_length", "max_length"],
    rich_text: ["max_length"],
    integer: ["min", "max"],
    decimal: ["min", "max", "max_scale"],
    boolean: [],
    date: ["min", "max"],
    date_time: ["min", "max"],
    url: ["schemes", "domains"],
    money: ["currencies", "min", "max"],
    measurement: ["dimension", "min", "max"],
    choice: ["choices"],
    reference: ["kind"],
    json: [],
  };
  return isList ? [...own[base], "min_items", "max_items", "unique_items"] : own[base];
}

/** `unique`: single text, integer and URL fields only. */
export function mayBeUnique(type: string): boolean {
  const { base, isList } = parseType(type);
  return !isList && (base === "text" || base === "integer" || base === "url");
}

/** `filterable`: anything with an index form; not long text, rich text or JSON. */
export function mayBeFilterable(type: string): boolean {
  const { base } = parseType(type);
  return base !== "multiline_text" && base !== "rich_text" && base !== "json";
}

export type Dimension = "weight" | "length" | "volume";

export const DIMENSIONS: { value: Dimension; label: string }[] = [
  { value: "weight", label: "Weight" },
  { value: "length", label: "Length" },
  { value: "volume", label: "Volume" },
];

export const UNITS: Record<Dimension, string[]> = {
  weight: ["g", "kg", "oz", "lb"],
  length: ["mm", "cm", "m", "in", "ft"],
  volume: ["ml", "l", "fl_oz"],
};

const isDimension = (value: string | null | undefined): value is Dimension =>
  value === "weight" || value === "length" || value === "volume";

export function unitsOf(dimension: string | null | undefined): string[] {
  return isDimension(dimension) ? UNITS[dimension] : UNITS.weight;
}

/** A JSON scalar as text; anything else as "". */
export function scalarText(value: unknown): string {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean"
    ? String(value)
    : "";
}

export const REFERENCE_KINDS = [
  { value: "product", label: "Product" },
  { value: "variant", label: "Variant" },
  { value: "category", label: "Category" },
  { value: "customer", label: "Customer" },
  { value: "asset", label: "Media file" },
] as const;

export const URL_SCHEMES = ["https", "http", "mailto", "tel"];

/** Who manages a definition: the merchant (`null`) or an app or spore by key. */
export function managerLabel(definition: Definition): string | null {
  const key = definition.managed_by;
  if (!key) return null;
  if (key.startsWith("spore_")) return `Spore ${key.slice(6)}`;
  if (key.startsWith("app_")) return `App ${key.slice(4)}`;
  return key;
}

/**
 * Whether the signed-in staff member may change values of `definition` (§5.1): the
 * owner's write scope, and for an app's or spore's field, `access.admin` `read_write`.
 */
export function isWritable(
  definition: Definition,
  ownerType: OwnerType,
  scopes: readonly string[] | null,
): boolean {
  if (scopes && !hasScope(scopes, ownerOf(ownerType).write)) return false;
  return !definition.managed_by || definition.access.admin === "read_write";
}

// ---------- values and inputs ----------

/** What an input holds while someone types; `valueOf` turns it into the field's JSON. */
export type FieldInput =
  | { kind: "text"; text: string }
  | { kind: "boolean"; value: boolean | null }
  | { kind: "money"; amount: string; currency: string }
  | { kind: "measurement"; value: string; unit: string }
  | { kind: "list"; items: string[] };

type Json = unknown;

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** A date-time as `YYYY-MM-DDTHH:mm` in UTC, as the date-time input takes it. */
function utcMinutes(rfc3339: string): string {
  const at = new Date(rfc3339);
  return Number.isNaN(at.getTime()) ? "" : at.toISOString().slice(0, 16);
}

/** One item of a list (or a scalar) as text: money as "12.50 EUR", measurements "1.5 kg". */
function itemText(base: BaseType, value: Json, currencies: Currencies): string {
  if (value === null || value === undefined) return "";
  if (base === "money" && isObject(value)) {
    const money = { amount: Number(value.amount), currency: scalarText(value.currency) };
    return currencies[money.currency] === undefined
      ? `${money.amount} ${money.currency}`
      : `${moneyInput(money, currencies)} ${money.currency}`;
  }
  if (base === "measurement" && isObject(value))
    return `${scalarText(value.value)} ${scalarText(value.unit)}`;
  if (base === "date_time" && typeof value === "string") return utcMinutes(value);
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}

/** The input for a stored value (`undefined`: none). `currency` is the default for money. */
export function inputOf(
  definition: Definition,
  value: Json,
  currencies: Currencies,
  currency: string,
): FieldInput {
  const { base, isList } = parseType(definition.type);
  if (isList) {
    return {
      kind: "list",
      items: Array.isArray(value) ? value.map((v) => itemText(base, v, currencies)) : [],
    };
  }
  switch (base) {
    case "boolean":
      return { kind: "boolean", value: typeof value === "boolean" ? value : null };
    case "money": {
      const allowed = definition.validations.currencies ?? [];
      const fallback = allowed.includes(currency) || allowed.length === 0 ? currency : allowed[0];
      if (isObject(value) && typeof value.currency === "string") {
        const known = currencies[value.currency] !== undefined;
        return {
          kind: "money",
          amount: known
            ? moneyInput({ amount: Number(value.amount), currency: value.currency }, currencies)
            : scalarText(value.amount),
          currency: value.currency,
        };
      }
      return { kind: "money", amount: "", currency: fallback };
    }
    case "measurement": {
      const units = unitsOf(definition.validations.dimension);
      if (isObject(value)) {
        return {
          kind: "measurement",
          value: scalarText(value.value),
          unit: scalarText(value.unit),
        };
      }
      return { kind: "measurement", value: "", unit: units[0] };
    }
    case "json":
      return {
        kind: "text",
        text: value === undefined || value === null ? "" : JSON.stringify(value, null, 2),
      };
    default:
      return { kind: "text", text: itemText(base, value, currencies) };
  }
}

type Parsed = { value: Json } | { error: string };

/** One scalar of `base` from text; blank text is never passed here. */
function parseItem(
  definition: Definition,
  base: BaseType,
  text: string,
  currencies: Currencies,
): Parsed {
  const t = text.trim();
  switch (base) {
    case "integer": {
      if (!/^-?\d+$/.test(t)) return { error: "Enter a whole number." };
      const n = Number(t);
      return Number.isSafeInteger(n) ? { value: n } : { error: "That number is too large." };
    }
    case "decimal":
      return /^-?\d+(\.\d+)?$/.test(t)
        ? { value: t }
        : { error: "Enter a number like 12.50 (a point for decimals)." };
    case "date":
      return /^\d{4}-\d{2}-\d{2}$/.test(t)
        ? { value: t }
        : { error: "Enter a date like 2026-10-20." };
    case "date_time": {
      const at = new Date(/[zZ]|[+-]\d\d:\d\d$/.test(t) ? t : `${t}Z`);
      return Number.isNaN(at.getTime())
        ? { error: "Enter a date and time." }
        : { value: at.toISOString().replace(".000Z", "Z") };
    }
    case "money": {
      const match = /^(\S+)\s+([A-Za-z]{3})$/.exec(t);
      if (!match) return { error: "Write each amount with its currency, like 12.50 EUR." };
      const currency = match[2].toUpperCase();
      if (currencies[currency] === undefined) return { error: `${currency} isn't a currency.` };
      const amount = parseMoney(match[1], currency, currencies);
      return amount === null
        ? { error: `Enter the amount in ${currency} as a number, like 18.50.` }
        : { value: { amount, currency } };
    }
    case "measurement": {
      const match = /^(-?\d+(?:\.\d+)?)\s*([a-z_]+)$/.exec(t);
      const units = unitsOf(definition.validations.dimension);
      if (!match || !units.includes(match[2])) {
        return {
          error: `Write each measurement with its unit (${units.join(", ")}), like 1.5 ${units[0]}.`,
        };
      }
      return { value: { value: match[1], unit: match[2] } };
    }
    case "choice": {
      const choices = definition.validations.choices ?? [];
      const found = choices.find((c) => c.key === t || c.label.toLowerCase() === t.toLowerCase());
      return found ? { value: found.key } : { error: `"${t}" isn't one of the choices.` };
    }
    default:
      return { value: t };
  }
}

/**
 * The JSON value an input stands for: `null` when it's empty (the field's value is then
 * removed), or what to fix. The API checks the field's own validations again.
 */
export function valueOf(definition: Definition, input: FieldInput, currencies: Currencies): Parsed {
  const { base } = parseType(definition.type);
  switch (input.kind) {
    case "boolean":
      return { value: input.value };
    case "list": {
      const values: Json[] = [];
      for (const item of input.items) {
        if (item.trim() === "") continue;
        const parsed = parseItem(definition, base, item, currencies);
        if ("error" in parsed) return parsed;
        values.push(parsed.value);
      }
      return { value: values.length === 0 ? null : values };
    }
    case "money": {
      if (input.amount.trim() === "") return { value: null };
      const amount = parseMoney(input.amount, input.currency, currencies);
      if (amount === null) {
        return { error: `Enter the amount in ${input.currency} as a number, like 18.50.` };
      }
      return { value: { amount, currency: input.currency } };
    }
    case "measurement": {
      if (input.value.trim() === "") return { value: null };
      return /^-?\d+(\.\d+)?$/.test(input.value.trim())
        ? { value: { value: input.value.trim(), unit: input.unit } }
        : { error: "Enter the measurement as a number, like 1.5." };
    }
    case "text": {
      if (base === "json") {
        if (input.text.trim() === "") return { value: null };
        try {
          return { value: JSON.parse(input.text) as Json };
        } catch {
          return { error: "That isn't valid JSON." };
        }
      }
      // Text keeps what was typed (the API decides about blanks inside); empty removes.
      if (input.text.trim() === "") return { value: null };
      if (base === "text" || base === "multiline_text" || base === "rich_text") {
        return { value: input.text };
      }
      return parseItem(definition, base, input.text, currencies);
    }
  }
}

/** `value` as JSON with every object's keys sorted: the API stores values as jsonb,
 *  which hands objects back in its own key order (`{unit, value}` for `{value, unit}`). */
function canonical(value: unknown): string {
  return JSON.stringify(value ?? null, (_key, v: unknown) =>
    v !== null && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(Object.entries(v).toSorted(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))
      : v,
  );
}

const same = (a: unknown, b: unknown) => canonical(a) === canonical(b);

/**
 * What saving `inputs` sends (§4): the merge patch of the fields that changed from
 * `saved`, each guarded by the version it was read at, or the problems by key.
 */
export function fieldsPatch(
  definitions: Definition[],
  inputs: Record<string, FieldInput>,
  saved: FieldsResponse,
  currencies: Currencies,
):
  | {
      patch: { custom_fields: Record<string, unknown>; if_versions: Record<string, number> } | null;
    }
  | { problems: Record<string, string> } {
  const custom_fields: Record<string, unknown> = {};
  const if_versions: Record<string, number> = {};
  const problems: Record<string, string> = {};
  for (const definition of definitions) {
    const input = inputs[definition.field];
    if (!input) continue;
    const parsed = valueOf(definition, input, currencies);
    if ("error" in parsed) {
      problems[definition.field] = parsed.error;
      continue;
    }
    const before = (saved.custom_fields as Record<string, unknown>)[definition.field];
    if (same(parsed.value, before)) continue;
    custom_fields[definition.field] = parsed.value;
    if_versions[definition.field] = saved.versions[definition.field] ?? 0;
  }
  if (Object.keys(problems).length > 0) return { problems };
  return { patch: Object.keys(custom_fields).length > 0 ? { custom_fields, if_versions } : null };
}

/** Whether any input differs from what's saved (an unparsable one counts as changed). */
export function fieldsChanged(
  definitions: Definition[],
  inputs: Record<string, FieldInput>,
  saved: FieldsResponse,
  currencies: Currencies,
): boolean {
  return definitions.some((definition) => {
    const input = inputs[definition.field];
    if (!input) return false;
    const parsed = valueOf(definition, input, currencies);
    if ("error" in parsed) return true;
    return !same(parsed.value, (saved.custom_fields as Record<string, unknown>)[definition.field]);
  });
}

/** Every field's input from the saved values. */
export function inputsOf(
  definitions: Definition[],
  saved: FieldsResponse,
  currencies: Currencies,
  currency: string,
): Record<string, FieldInput> {
  const values = saved.custom_fields as Record<string, unknown>;
  return Object.fromEntries(
    definitions.map((d) => [d.field, inputOf(d, values[d.field], currencies, currency)]),
  );
}

/**
 * The problems of a `422 custom_fields_invalid` by field key (`details`: `[{key, path,
 * code, message}]`), or of a `409 custom_field_stale` (`{key}`).
 */
export function problemsOf(code: string | undefined, details: unknown): Record<string, string> {
  const problems: Record<string, string> = {};
  if (code === "custom_fields_invalid" && Array.isArray(details)) {
    for (const item of details) {
      if (!isObject(item) || typeof item.key !== "string") continue;
      const message = typeof item.message === "string" ? item.message : "isn't valid";
      const path = typeof item.path === "string" && item.path !== item.key ? `${item.path}: ` : "";
      const text = `${path}${message.charAt(0).toUpperCase()}${message.slice(1)}`;
      problems[item.key] = problems[item.key] ? `${problems[item.key]} ${text}` : text;
    }
  }
  if (code === "custom_field_stale" && isObject(details) && typeof details.key === "string") {
    problems[details.key] = "Someone else changed this field meanwhile. Reload to see it.";
  }
  return problems;
}

// ---------- list filters (§7) ----------

/** A list filter's URL parameter for a field: `field[custom.material]`. */
export function filterParam(definition: Definition): string {
  return `field[${definition.field}]`;
}

/** Types offered as list filters: equality on text-like values, true/false, choices. */
export function isFilterOffered(definition: Definition): boolean {
  if (!definition.capabilities.filterable) return false;
  const { base } = parseType(definition.type);
  return ["text", "url", "integer", "decimal", "boolean", "choice", "reference", "date"].includes(
    base,
  );
}

/**
 * The API's `field` query from a list page's URL: each offered filter's values (several
 * of a choice are "any of", comma-separated), keyed as openapi-fetch writes them into
 * `field[key]=…`. Only fields in `definitions`, so a hand-edited URL can't send others.
 */
export function fieldQuery(
  params: Record<string, string | string[] | undefined>,
  definitions: Definition[],
): Record<string, string> | undefined {
  const query: Record<string, string> = {};
  for (const definition of definitions) {
    if (!isFilterOffered(definition)) continue;
    const raw = params[filterParam(definition)];
    const values = (Array.isArray(raw) ? raw : raw === undefined ? [] : [raw])
      .map((v) => v.trim())
      .filter((v) => v !== "" && !v.includes(","));
    if (values.length > 0) query[definition.field] = values.join(",");
  }
  return Object.keys(query).length > 0 ? query : undefined;
}
