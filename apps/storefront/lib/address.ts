// Address forms' types, their labels in the page's language, and the problems an
// `invalid_address` answer carries. Safe on both sides: no API calls here.

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import type { KeyIn, TFor } from "./messages.ts";

/** A country's address form (`GET /storefront/address-formats/{country}`). */
export type AddressFormat = StorefrontSchemas["AddressFormatResponse"];

/** The fields an address has, as the API names them. */
export const ADDRESS_FIELDS = [
  "first_name",
  "last_name",
  "company",
  "line1",
  "line2",
  "city",
  "province",
  "postal_code",
  "country",
  "phone",
] as const;

export type AddressField = (typeof ADDRESS_FIELDS)[number];

/** A field the API named, if this build knows it (the API may add fields). */
export function asField(name: unknown): AddressField | undefined {
  return ADDRESS_FIELDS.find((f) => f === name);
}

/** The required fields of `format` that are empty in `values`. */
export function missingIn(format: AddressFormat, values: AddressValues): AddressField[] {
  return format.required
    .map(asField)
    .filter((f): f is Exclude<AddressField, "country"> => f !== undefined && f !== "country")
    .filter((f) => values[f].trim() === "");
}

/** An address as typed: every field but the country, blank for none. */
export type AddressValues = Record<Exclude<AddressField, "country">, string>;

/** What a client words a built-in message or problem from (`MessageParamsDto`). */
export type MessageParams = StorefrontSchemas["MessageParamsDto"];

/** One field's problem: worded from `code` and `params` where they're built in
 * (`wordMessage`), else its `message`. */
export type FieldProblem = { code: string; message: string; params?: MessageParams | null };

/** Each field's problem, by field. */
export type FieldProblems = Partial<Record<AddressField, FieldProblem>>;

/** The catalog's message for each of the API's label keys (`label_keys`). */
const LABELS = {
  first_name: "address.label_first_name",
  last_name: "address.label_last_name",
  company: "address.label_company",
  address1: "address.label_address1",
  address2: "address.label_address2",
  street_and_house_number: "address.label_street_and_house_number",
  full_address: "address.label_full_address",
  additional_address: "address.label_additional_address",
  city: "address.label_city",
  city_town: "address.label_city_town",
  city_ward_town_village: "address.label_city_ward_town_village",
  ward: "address.label_ward",
  district: "address.label_district",
  suburb: "address.label_suburb",
  area: "address.label_area",
  region: "address.label_region",
  province: "address.label_province",
  state: "address.label_state",
  state_territory: "address.label_state_territory",
  county: "address.label_county",
  prefecture: "address.label_prefecture",
  emirate: "address.label_emirate",
  governorate: "address.label_governorate",
  department: "address.label_department",
  postal_code: "address.label_postal_code",
  postcode: "address.label_postcode",
  zip_code: "address.label_zip_code",
  pin_code: "address.label_pin_code",
  phone: "address.label_phone",
} as const satisfies Record<string, KeyIn<"address">>;

/** A label key this build has a message for. */
function isLabelKey(key: string): key is keyof typeof LABELS {
  return Object.hasOwn(LABELS, key);
}

/**
 * A field's label in the page's language: the catalog's for the format's label key, or
 * the format's own (English) label where the key is `null` or one this build doesn't
 * know. The country, which forms show apart, has a message of its own.
 */
export function fieldLabel(t: TFor<"address">, format: AddressFormat, field: string): string {
  const key = format.label_keys[field];
  if (key != null && isLabelKey(key)) return t(LABELS[key]);
  if (field === "country") return t("address.label_country");
  return format.labels[field] ?? field;
}

/** A label as it reads inside a sentence: "State" becomes "state", while "ZIP code"
 * stays as it is (the API's own rule). */
export function inSentence(label: string, language: string): string {
  const [first, second] = label;
  if (first === undefined) return label;
  if (second !== undefined && second !== second.toLocaleLowerCase(language)) return label;
  return first.toLocaleLowerCase(language) + label.slice(first.length);
}

/** The problems of an `invalid_address` error, by field; none for another error. */
export function problemsOf(error: { code: string; details?: unknown }): FieldProblems | undefined {
  if (error.code !== "invalid_address") return undefined;
  const details = error.details;
  if (typeof details !== "object" || details === null) return undefined;
  const list: unknown = Reflect.get(details, "problems");
  if (!Array.isArray(list)) return undefined;
  const problems: FieldProblems = {};
  for (const item of list) {
    if (typeof item !== "object" || item === null) continue;
    const field = asField(Reflect.get(item, "field"));
    const problem = asProblem(item);
    if (field && problem) problems[field] ??= problem;
  }
  return problems;
}

/** A problem as the API sent it, if it has a code and a message. */
export function asProblem(item: object): FieldProblem | undefined {
  const code: unknown = Reflect.get(item, "code");
  const message: unknown = Reflect.get(item, "message");
  if (typeof code !== "string" || typeof message !== "string") return undefined;
  const params = asParams(Reflect.get(item, "params"));
  return params ? { code, message, params } : { code, message };
}

/** The params of a message the API sent untyped (an error's `details`): what's there
 * of the known ones. */
export function asParams(value: unknown): MessageParams | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const params: MessageParams = {};
  const text = (key: string) => {
    const found: unknown = Reflect.get(value, key);
    return typeof found === "string" ? found : undefined;
  };
  const field = text("field");
  if (field !== undefined) params.field = field;
  const example = text("example");
  if (example !== undefined) params.example = example;
  const region = text("region");
  if (region !== undefined) params.region = region;
  const regionName = text("region_name");
  if (regionName !== undefined) params.region_name = regionName;
  const max: unknown = Reflect.get(value, "max");
  if (typeof max === "number") params.max = max;
  return params;
}
