// Address forms' types and the problems an `invalid_address` answer carries. Safe on
// both sides: no API calls here.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

/** A country's address form (`GET /admin/address-formats/{country}`). */
export type AddressFormat = AdminSchemas["AddressFormatResponse"];

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

/** Each field's problem message, by field. */
export type FieldProblems = Partial<Record<AddressField, string>>;

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
    const name: unknown = Reflect.get(item, "field");
    const message: unknown = Reflect.get(item, "message");
    const field = asField(name);
    if (field && typeof message === "string") problems[field] ??= message;
  }
  return problems;
}
