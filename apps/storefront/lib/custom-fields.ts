// Custom fields as shoppers read them: a product's, a variant's or a category's
// `custom_fields` (`{"custom.material": "wool"}`), only those the merchant shows
// storefronts. Their definitions (`/storefront/custom-field- definitions`) give each
// field its name, its place and a choice's label; a value is shown by its shape: a
// boolean as Yes/No, a date in the store's language, money with its currency's minor
// units, a measurement as "1.5 kg", a URL as a link, rich text as HTML (when it's the
// sanitised kind), a reference to a product or a category as a link to its page, a list
// joined; JSON isn't shown.
//
// Formatting happens on the server (`presentFields`), so the product form, a client
// component, gets plain strings and never formats in the browser (no hydration
// mismatch from a different ICU).

import { formatMoney } from "./money";

/** One piece of a value: text, or a link. */
export type FieldPart = { text: string; href?: string };

/** A part in its value's list: `key` tells it from the others (a list may repeat a
 * value). */
export type KeyedPart = FieldPart & { key: string };

/** A field ready to show: its label, then rich text (`html`, sanitised by the API and
 * checked here), multiline text (`lines`), or parts joined with commas. */
export type FieldEntry = {
  key: string;
  label: string;
  html?: string;
  lines?: string;
  parts?: KeyedPart[];
};

export type FieldContext = {
  /** The store's language (`default_language`), for dates and numbers. */
  locale: string;
  /** The shopper's country's currency and its minor units, for money in it. */
  currency: string;
  digits: number;
  /** Product and category references the page could resolve, by ID. */
  refs: Record<string, FieldPart>;
  /** The definitions storefronts are given, by `<owner type>:<field>` (keys are unique
   * per owner type): a name, a place, a choice's labels by key. */
  definitions: Record<string, FieldDefinition>;
};

export type FieldDefinition = {
  name: string;
  position: number;
  /** `text`, `choice`, `list.choice`, … */
  type: string;
  choices: Record<string, string>;
};

/** Whose values: what `custom_fields` belongs to. */
export type FieldOwner = "product" | "variant" | "category" | "customer";

/** A prefixed ID (`prod_…`, `pcat_…`), as a `reference` field holds one. */
const REFERENCE = /^([a-z]+)_[0-9a-f]{32}$/;
/** Kinds whose references link to a storefront page. */
const LINKED_KINDS = new Set(["prod", "pcat"]);
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const DATE_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})$/i;
const URL_VALUE = /^(https?:\/\/|mailto:|tel:)\S+$/i;
/** A `choice` key of a field without a definition (a cached page of an older one):
 * shown humanised. */
const CHOICE_KEY = /^[a-z][a-z0-9]*(?:[_-][a-z0-9]+)*$/;

/** `custom.size_chart` → "Size chart"; the namespace isn't shown. */
export function fieldLabel(key: string): string {
  const name = key.includes(".") ? key.slice(key.indexOf(".") + 1) : key;
  return humanise(name);
}

/** `dry_skin` → "Dry skin". */
function humanise(word: string): string {
  const spaced = word.replaceAll("_", " ").trim();
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/** A locale `Intl` takes: the store's language, else English. */
function localeOf(language: string): string {
  try {
    return Intl.getCanonicalLocales(language)[0] ?? "en";
  } catch {
    return "en";
  }
}

/** The references (product and category IDs) among `values`, for the page to resolve. */
export function referenceIds(...values: Record<string, unknown>[]): string[] {
  const found = new Set<string>();
  const visit = (value: unknown) => {
    if (Array.isArray(value)) value.forEach(visit);
    else if (typeof value === "string") {
      const kind = REFERENCE.exec(value)?.[1];
      if (kind && LINKED_KINDS.has(kind)) found.add(value);
    }
  };
  for (const map of values) Object.values(map).forEach(visit);
  return [...found];
}

// ---------- rich text ----------

/** The tags the API's sanitiser keeps that a field shows as HTML. Anything else, and a
 * value that isn't exactly this markup, is shown as text (React escapes it). */
const TAGS = new Set([
  "a",
  "abbr",
  "b",
  "blockquote",
  "br",
  "code",
  "dd",
  "del",
  "div",
  "dl",
  "dt",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "i",
  "ins",
  "li",
  "mark",
  "ol",
  "p",
  "pre",
  "q",
  "s",
  "small",
  "span",
  "strike",
  "strong",
  "sub",
  "sup",
  "table",
  "tbody",
  "td",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
]);
const ATTRIBUTES = new Set(["href", "title", "rel", "lang", "dir", "colspan", "rowspan", "start"]);
const TAG = /<(\/?)([a-zA-Z][a-zA-Z0-9]*)((?:\s+[a-z-]+="[^"<>]*")*)\s*(\/?)>/g;
const ATTRIBUTE = /\s+([a-z-]+)="([^"<>]*)"/g;

/** Whether an `href` is safe: an http(s), mailto or tel URL, or a relative one. The
 * scheme is read literally, and no entity may hide one (`&#58;`). */
function safeHref(value: string): boolean {
  if (/&(?!amp;)/.test(value)) return false;
  const scheme = /^([^/?#]*):/.exec(value)?.[1];
  return scheme === undefined || /^(https?|mailto|tel)$/i.test(scheme);
}

/**
 * Whether `value` is rich text this page may render as HTML: markup made only of the
 * tags and attributes above, with no other `<`. The API sanitises `rich_text` values
 * already (like descriptions); this tells them from plain text, which isn't sanitised
 * and must never become HTML.
 */
export function isRichText(value: string): boolean {
  let tags = 0;
  let safe = true;
  const rest = value.replace(TAG, (_tag, _close: string, name: string, attributes: string) => {
    tags += 1;
    if (!TAGS.has(name.toLowerCase())) safe = false;
    for (const [, attribute, attributeValue] of attributes.matchAll(ATTRIBUTE)) {
      if (!ATTRIBUTES.has(attribute)) safe = false;
      if (attribute === "href" && !safeHref(attributeValue)) safe = false;
    }
    return "";
  });
  return safe && tags > 0 && !rest.includes("<") && !rest.includes(">");
}

// ---------- values ----------

type Money = { amount: number; currency: string };
type Measurement = { value: string; unit: string };

function isMoney(value: object): value is Money {
  return (
    Object.keys(value).length === 2 &&
    "amount" in value &&
    typeof value.amount === "number" &&
    Number.isSafeInteger(value.amount) &&
    "currency" in value &&
    typeof value.currency === "string" &&
    /^[A-Z]{3}$/.test(value.currency)
  );
}

function isMeasurement(value: object): value is Measurement {
  return (
    Object.keys(value).length === 2 &&
    "value" in value &&
    typeof value.value === "string" &&
    "unit" in value &&
    typeof value.unit === "string"
  );
}

/** A currency's minor units, as `Intl` knows them (the country's own when it's theirs). */
function digitsOf(currency: string, context: FieldContext): number {
  if (currency === context.currency) return context.digits;
  try {
    return (
      new Intl.NumberFormat("en", { style: "currency", currency }).resolvedOptions()
        .maximumFractionDigits ?? 2
    );
  } catch {
    return 2;
  }
}

/** Formatters by locale (and kind), built once: a page formats many values. */
const dateFormats = new Map<string, Intl.DateTimeFormat>();
const numberFormats = new Map<string, Intl.NumberFormat>();

function dateFormat(locale: string, withTime: boolean): Intl.DateTimeFormat {
  const id = `${withTime}:${locale}`;
  let made = dateFormats.get(id);
  if (!made) {
    made = new Intl.DateTimeFormat(locale, {
      dateStyle: "long",
      timeStyle: withTime ? "short" : undefined,
      timeZone: "UTC",
    });
    dateFormats.set(id, made);
  }
  return made;
}

function numberFormat(locale: string): Intl.NumberFormat {
  let made = numberFormats.get(locale);
  if (!made) {
    made = new Intl.NumberFormat(locale);
    numberFormats.set(locale, made);
  }
  return made;
}

/** A string value: a reference, a link, a date or a time, a choice's key, or text. */
function stringPart(
  value: string,
  context: FieldContext,
  definition?: FieldDefinition,
): FieldPart | null {
  if (definition) {
    const label = definition.choices[value];
    if (label !== undefined) return { text: label };
  }
  if (REFERENCE.test(value)) return context.refs[value] ?? null;
  if (URL_VALUE.test(value)) {
    return { text: value.replace(/^(mailto|tel):/i, ""), href: value };
  }
  const locale = localeOf(context.locale);
  if (DATE.test(value)) {
    const at = new Date(`${value}T00:00:00Z`);
    if (!Number.isNaN(at.getTime())) {
      return { text: dateFormat(locale, false).format(at) };
    }
  }
  if (DATE_TIME.test(value)) {
    const at = new Date(value);
    if (!Number.isNaN(at.getTime())) {
      // In UTC, said so: the storefront doesn't know the store's time zone.
      return { text: `${dateFormat(locale, true).format(at)} UTC` };
    }
  }
  // Only guess a choice when there's no definition to say so.
  if (!definition && CHOICE_KEY.test(value) && value.length <= 64) {
    return { text: humanise(value) };
  }
  return { text: value };
}

/** One value (or one item of a list) as a part, or `null` when it isn't shown. */
function part(
  value: unknown,
  context: FieldContext,
  definition?: FieldDefinition,
): FieldPart | null {
  if (typeof value === "boolean") return { text: value ? "Yes" : "No" };
  if (typeof value === "number") {
    return { text: numberFormat(localeOf(context.locale)).format(value) };
  }
  if (typeof value === "string") return stringPart(value, context, definition);
  if (value && typeof value === "object" && !Array.isArray(value)) {
    if (isMoney(value)) {
      return {
        text: formatMoney(value, digitsOf(value.currency, context), localeOf(context.locale)),
      };
    }
    if (isMeasurement(value)) return { text: `${value.value} ${value.unit.replaceAll("_", " ")}` };
  }
  // JSON (or anything else): not shown.
  return null;
}

/** What `value` shows, or `null` for nothing (JSON, an unresolved reference, empty). */
function present(
  key: string,
  value: unknown,
  context: FieldContext,
  definition?: FieldDefinition,
): FieldEntry | null {
  const label = definition?.name ?? fieldLabel(key);
  if (typeof value === "string") {
    if (value.trim() === "") return null;
    if (value.includes("<") && isRichText(value)) return { key, label, html: value };
    if (value.includes("\n")) return { key, label, lines: value };
  }
  const items = Array.isArray(value) ? value : [value];
  const parts = items
    .map((item) => (Array.isArray(item) ? null : part(item, context, definition)))
    .filter((p): p is FieldPart => p !== null)
    // A list's items never move, so their places are stable keys.
    .map((p, place) => ({ ...p, key: `${place}` }));
  return parts.length > 0 ? { key, label, parts } : null;
}

/** An owner's `custom_fields` as entries in the merchant's order (fields without a
 * definition last, as the API sent them), leaving out what isn't shown. */
export function presentFields(
  owner: FieldOwner,
  values: Record<string, unknown> | undefined,
  context: FieldContext,
): FieldEntry[] {
  const definitionOf = (key: string) => context.definitions[`${owner}:${key}`];
  const place = (key: string) => definitionOf(key)?.position ?? Number.MAX_SAFE_INTEGER;
  return Object.entries(values ?? {})
    .toSorted(([a], [b]) => place(a) - place(b))
    .map(([key, value]) => present(key, value, context, definitionOf(key)))
    .filter((e): e is FieldEntry => e !== null);
}

/** A variant's entries over its product's: the variant's value wins for a key, in the
 * product's place; keys only the variant has come after. */
export function withVariant(product: FieldEntry[], variant: FieldEntry[] = []): FieldEntry[] {
  const own = new Map(variant.map((e) => [e.key, e]));
  const merged = product.map((e) => own.get(e.key) ?? e);
  const shown = new Set(product.map((e) => e.key));
  return [...merged, ...variant.filter((e) => !shown.has(e.key))];
}
