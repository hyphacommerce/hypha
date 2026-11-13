// Translating a resource's text: which kinds there are and what reading one takes, what
// a field's key stands for, and the editor's draft and what it saves. Plain functions,
// on both sides and under `node --test`; the reads are in `translations-server.ts`.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

export type Translations = AdminSchemas["TranslationsResponse"];
export type TranslatableField = AdminSchemas["TranslatableFieldResponse"];
export type PutTranslation = AdminSchemas["PutTranslationRequest"];

/** The kinds of resource with translatable text, as the API names them. */
export const KINDS = [
  "product",
  "category",
  "custom_field_definition",
  "shipping_option",
  "payment_method",
  "discount",
  "checkout_rule",
  "asset",
  "store",
  "search_facet",
] as const;

export type Kind = (typeof KINDS)[number];

export function isKind(value: string): value is Kind {
  return KINDS.some((kind) => kind === value);
}

/** What reading the resource itself takes, beside `translations:read`. */
const READ_SCOPES: Record<Kind, readonly string[]> = {
  product: ["products:read"],
  category: ["products:read"],
  search_facet: ["products:read"],
  custom_field_definition: ["custom_fields:read"],
  shipping_option: ["shipping:read"],
  payment_method: ["payments:read"],
  discount: ["discounts:read"],
  checkout_rule: ["checkout_rules:read"],
  asset: ["media:read"],
  store: ["stores:read", "notifications:read"],
};

/** A write scope implies its read scope, as in the API. */
function holds(scopes: readonly string[], scope: string): boolean {
  return (
    scopes.includes(scope) ||
    (scope.endsWith(":read") && scopes.includes(scope.replace(/:read$/, ":write")))
  );
}

/** Whether a staff member with `scopes` can open the translations of a `kind`. */
export function canReadTranslations(scopes: readonly string[], kind: Kind): boolean {
  return (
    holds(scopes, "translations:read") && READ_SCOPES[kind].every((scope) => holds(scopes, scope))
  );
}

/** Whether they can also save them. */
export function canWriteTranslations(scopes: readonly string[], kind: Kind): boolean {
  return scopes.includes("translations:write") && canReadTranslations(scopes, kind);
}

/** The editor's page for a resource, opened on `language` when given. */
export function translateHref(kind: Kind, id: string, language?: string): string {
  const path = `/translations/${kind}/${encodeURIComponent(id)}`;
  return language ? `${path}?language=${encodeURIComponent(language)}` : path;
}

/** The page of the resource itself: where the editor's breadcrumb goes back to. */
export function resourceHref(kind: Kind, id: string): string {
  switch (kind) {
    case "product":
      return `/products/${id}`;
    case "category":
      return `/products/categories/${id}`;
    case "custom_field_definition":
      return "/custom-fields";
    case "shipping_option":
      return "/settings/shipping";
    case "payment_method":
      return "/settings/payments";
    case "discount":
      return `/discounts/${id}`;
    case "checkout_rule":
      return `/settings/checkout/rules/${id}`;
    case "asset":
      return `/media/${id}`;
    case "store":
    case "search_facet":
      return "/settings/languages";
  }
}

/** The editor's sections, in the order they show. */
export const GROUPS = ["details", "options", "option_values", "choices", "facets"] as const;
export type Group = (typeof GROUPS)[number];

/**
 * What a field's key stands for. A plain field is named by its key (`title`); a child's
 * by the child's own source text, which is what a translator recognises.
 */
export type FieldName =
  /** `title`, `description`, `email_footer`…: a label per name, the raw key otherwise. */
  | { type: "plain"; name: string }
  /** `options.<id>.name`: "Option: Size". */
  | { type: "option"; source: string }
  /** `option_values.<id>.label`: "Size: Large"; `option` is absent when it isn't known. */
  | { type: "option_value"; option?: string; source: string }
  /** `choices.<key>.label`: "Choice: Cotton". */
  | { type: "choice"; source: string }
  /** `facets.<key>.label`: "Filter: Colour". */
  | { type: "facet"; source: string };

export type DescribedField = { group: Group; name: FieldName };

/**
 * The section and name of the field under `key`. `source` is the field's own source
 * text; `optionOf` names the option an option value belongs to (the source name, by the
 * value's ID), where the page knows the product. A child whose source is blank is named
 * by its ID or key instead, so no label is ever empty.
 */
export function describeField(
  key: string,
  source: string,
  optionOf: (valueId: string) => string | undefined = () => undefined,
): DescribedField {
  const first = key.indexOf(".");
  const last = key.lastIndexOf(".");
  if (first === -1 || first === last)
    return { group: "details", name: { type: "plain", name: key } };
  const collection = key.slice(0, first);
  // A facet's key has dots of its own (`option.Color`): the child is everything between.
  const child = key.slice(first + 1, last);
  const field = key.slice(last + 1);
  const shown = source.trim() === "" ? child : source;
  if (collection === "options" && field === "name")
    return { group: "options", name: { type: "option", source: shown } };
  if (collection === "option_values" && field === "label")
    return {
      group: "option_values",
      name: { type: "option_value", option: optionOf(child), source: shown },
    };
  if (collection === "choices" && field === "label")
    return { group: "choices", name: { type: "choice", source: shown } };
  if (collection === "facets" && field === "label")
    return { group: "facets", name: { type: "facet", source: shown } };
  return { group: "details", name: { type: "plain", name: key } };
}

/** `fields` by section, in the sections' order, each keeping the API's order within. */
export function grouped<F extends { key: string; source: string }>(
  fields: readonly F[],
  optionOf?: (valueId: string) => string | undefined,
): { group: Group; fields: { field: F; name: FieldName }[] }[] {
  return GROUPS.map((group) => ({
    group,
    fields: fields.flatMap((field) => {
      const described = describeField(field.key, field.source, optionOf);
      return described.group === group ? [{ field, name: described.name }] : [];
    }),
  })).filter((section) => section.fields.length > 0);
}

/** One field as the editor holds it. */
export type DraftField = {
  value: string;
  /** Marked "reviewed": the translation still fits the source as it is now. */
  reviewed: boolean;
};

export type Draft = Partial<Record<string, DraftField>>;

/** The saved translation of each field into `language`: blank where there's none. */
export function draftOf(fields: readonly TranslatableField[], language: string): Draft {
  return Object.fromEntries(
    fields.map((field) => [
      field.key,
      { value: field.translations[language]?.value ?? "", reviewed: false },
    ]),
  );
}

/** A value with nothing in it: the API removes such a field, and the source shows. */
function isBlank(value: string): boolean {
  return value.trim() === "";
}

/** Whether the editor's value differs from what's saved (blank and absent are one). */
function isEdited(field: TranslatableField, language: string, entry: DraftField | undefined) {
  const saved = field.translations[language]?.value ?? "";
  const value = entry?.value ?? "";
  return isBlank(value) ? !isBlank(saved) : value !== saved;
}

/** Whether saving `draft` would change anything of `language`. */
export function isDirty(
  fields: readonly TranslatableField[],
  language: string,
  draft: Draft,
): boolean {
  return fields.some((field) => {
    const entry = draft[field.key];
    if (isEdited(field, language, entry)) return true;
    // Marking an up-to-date or absent translation reviewed changes nothing.
    return (entry?.reviewed ?? false) && (field.translations[language]?.outdated ?? false);
  });
}

/**
 * The body that saves `draft` as the whole of `language`: every field with text, since
 * one left out is removed. A field carries the source's `digest` when the translator
 * dealt with it against the source shown (typed a new value, or marked it reviewed):
 * the API then records it as up to date, and refuses the save (`source_changed`) if the
 * source moved meanwhile. An untouched field goes without one and stays as it was,
 * outdated if it was.
 */
export function putBody(
  fields: readonly TranslatableField[],
  language: string,
  draft: Draft,
): PutTranslation {
  const body: PutTranslation = { fields: {} };
  for (const field of fields) {
    const entry = draft[field.key];
    if (!entry || isBlank(entry.value)) continue;
    const touched = entry.reviewed || isEdited(field, language, entry);
    body.fields[field.key] = touched
      ? { value: entry.value, source_digest: field.digest }
      : { value: entry.value };
  }
  return body;
}

/**
 * A value's length as the field's limit counts it: characters (code points) for text,
 * bytes of UTF-8 for HTML.
 */
export function lengthOf(value: string, format: string): number {
  if (format === "html") return new TextEncoder().encode(value).length;
  // A string iterates by code point, which is how the API counts characters.
  return Array.from(value).length;
}

/** The longest text a one-line input takes; longer fields get a text area. */
export const SINGLE_LINE_MAX = 255;

/** A problem the API found with one field of a save. */
export type FieldProblem = { code: string; message: string };

/** An `invalid_translation` answer's problems (`details.problems`), by field key. */
export function problemsOf(details: unknown): Record<string, FieldProblem> {
  if (typeof details !== "object" || details === null) return {};
  const problems: unknown = Reflect.get(details, "problems");
  if (!Array.isArray(problems)) return {};
  const byKey: Record<string, FieldProblem> = {};
  for (const problem of problems as unknown[]) {
    if (typeof problem !== "object" || problem === null) continue;
    const key: unknown = Reflect.get(problem, "key");
    const code: unknown = Reflect.get(problem, "code");
    const message: unknown = Reflect.get(problem, "message");
    if (typeof key !== "string" || typeof code !== "string") continue;
    // The first problem of a field is the one shown.
    byKey[key] ??= { code, message: typeof message === "string" ? message : "" };
  }
  return byKey;
}
