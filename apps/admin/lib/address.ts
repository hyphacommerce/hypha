// Address forms' types, their fields' labels in the staff member's language, and the
// problems and messages the API reports about an address, worded the same way. Safe on
// both sides: no API calls here.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import type { TFor } from "./messages";

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

// Labels.

/**
 * What a field's label says, by meaning (the format's `label_keys`): each has a message,
 * `components.address_label_<key>`, whose English is the API's `labels` word for word.
 */
export const LABEL_KEYS = [
  "first_name",
  "last_name",
  "company",
  "address1",
  "address2",
  "street_and_house_number",
  "full_address",
  "additional_address",
  "city",
  "city_town",
  "city_ward_town_village",
  "ward",
  "district",
  "suburb",
  "area",
  "region",
  "province",
  "state",
  "state_territory",
  "county",
  "prefecture",
  "emirate",
  "governorate",
  "department",
  "postal_code",
  "postcode",
  "zip_code",
  "pin_code",
  "phone",
] as const;

export type LabelKey = (typeof LABEL_KEYS)[number];

/** A label key the API sent, if this build knows it (the API adds keys). */
function asLabelKey(key: unknown): LabelKey | undefined {
  return LABEL_KEYS.find((k) => k === key);
}

/** Each field's label where no country names it: the API's defaults. */
const DEFAULT_LABELS: Record<Exclude<AddressField, "country">, LabelKey> = {
  first_name: "first_name",
  last_name: "last_name",
  company: "company",
  line1: "address1",
  line2: "address2",
  city: "city",
  province: "region",
  postal_code: "postal_code",
  phone: "phone",
};

/** What labels an address's fields need: the shared vocabulary for the country. */
export type AddressT = TFor<"components" | "common">;

/** What labels a country's fields: its format's `labels` and `label_keys`. */
export type LabelledFormat = Pick<AddressFormat, "labels" | "label_keys">;

/**
 * `field`'s label in `t`'s language: by the key the country's format gives it, its
 * English label as the API has it where the key is `null` or one this build doesn't
 * know, and the field's default label without a format (or where the format doesn't
 * label it). An unknown field is shown as it is.
 */
export function addressFieldLabel(
  t: AddressT,
  format: LabelledFormat | null | undefined,
  field: string,
): string {
  if (field === "country") return t("common.country");
  // An answer cached from before the API sent `label_keys` has only `labels`.
  const keys: Partial<AddressFormat["label_keys"]> = format?.label_keys ?? {};
  const key = Object.hasOwn(keys, field) ? asLabelKey(keys[field]) : undefined;
  if (key) return t(`components.address_label_${key}`);
  if (format && Object.hasOwn(format.labels, field)) return format.labels[field];
  const known = asField(field);
  if (known === undefined || known === "country") return field;
  return t(`components.address_label_${DEFAULT_LABELS[known]}`);
}

// Problems and messages.

/**
 * A built-in address problem or cart message as the API sent it: its code, the API's
 * English sentence, and what the admin words it from in the staff member's language
 * (`wordAddressMessage`). `params` is absent for a message stored before the API sent
 * them.
 */
export type AddressProblem = {
  code: string;
  message: string;
  params?: MessageParams | null;
};

export type MessageParams = AdminSchemas["MessageParamsDto"];

/** Each field's problem, by field. */
export type FieldProblems = Partial<Record<AddressField, AddressProblem>>;

/**
 * Who said a message: a checkout rule's and an app's are the merchant's or the app's
 * words, shown as written.
 */
type Source = { type: string };

/**
 * A problem's or a message's sentence in `t`'s language, from its `code` and `params`:
 * the API's `message` for a code this build doesn't know, for a checkout rule's or an
 * app's message, and for a code whose params are missing. A field is named as `format`
 * labels it (`addressFieldLabel`).
 */
export function wordAddressMessage(
  t: AddressT,
  message: AddressProblem & { source?: Source },
  format: LabelledFormat | null | undefined,
): string {
  const label = (field: string) => addressFieldLabel(t, format, field);
  if (
    message.source &&
    message.source.type !== "format" &&
    message.source.type !== "verification"
  ) {
    return message.message;
  }
  const params = message.params;
  const field = params?.field ?? undefined;
  switch (message.code) {
    case "required":
    case "unknown_region":
      return field
        ? t(`components.address_message_${message.code}`, { label: label(field) })
        : message.message;
    case "too_long":
      return field && typeof params?.max === "number"
        ? t("components.address_message_too_long", { label: label(field), max: params.max })
        : message.message;
    case "invalid_format":
      if (!field) return message.message;
      return params?.example
        ? t("components.address_message_invalid_format_example", {
            label: label(field),
            example: params.example,
          })
        : t("components.address_message_invalid_format", { label: label(field) });
    case "region_mismatch": {
      // The region's name, or its code where the API had no name for it.
      const region = params?.region_name ?? params?.region;
      return field && region
        ? t("components.address_message_region_mismatch", { label: label(field), region })
        : message.message;
    }
    case "house_number_missing":
    case "apartment_missing":
    case "address_corrections_suggested":
    case "address_ambiguous":
    case "address_undeliverable":
      return t(`components.address_message_${message.code}`);
    default:
      return message.message;
  }
}

/** A problem read from an answer: what the admin keeps of it. */
type ProblemIn = { field: string; code: string; message: string; params?: unknown };

/** The params a problem came with, read field by field (an error's `details` is untyped). */
function paramsOf(value: unknown): MessageParams | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const text = (key: string): string | undefined => {
    const v: unknown = Reflect.get(value, key);
    return typeof v === "string" ? v : undefined;
  };
  const max: unknown = Reflect.get(value, "max");
  const params: MessageParams = {};
  const field = text("field");
  if (field !== undefined) params.field = field;
  if (typeof max === "number") params.max = max;
  const example = text("example");
  if (example !== undefined) params.example = example;
  const region = text("region");
  if (region !== undefined) params.region = region;
  const regionName = text("region_name");
  if (regionName !== undefined) params.region_name = regionName;
  return params;
}

/**
 * Problems by their field, the first one a field has; fields this build doesn't know are
 * left out. Each keeps its code, sentence and params, with the field it's under in its
 * params where the API didn't put it there.
 */
export function problemsByField(list: readonly ProblemIn[]): FieldProblems {
  const problems: FieldProblems = {};
  for (const p of list) {
    const field = asField(p.field);
    if (!field || problems[field]) continue;
    const params = paramsOf(p.params);
    problems[field] = {
      code: p.code,
      message: p.message,
      params: params ? { ...params, field: params.field ?? field } : undefined,
    };
  }
  return problems;
}

/** The problems of an `invalid_address` error, by field; none for another error. */
export function problemsOf(error: { code: string; details?: unknown }): FieldProblems | undefined {
  if (error.code !== "invalid_address") return undefined;
  const details = error.details;
  if (typeof details !== "object" || details === null) return undefined;
  const list: unknown = Reflect.get(details, "problems");
  if (!Array.isArray(list)) return undefined;
  const items: ProblemIn[] = [];
  for (const item of list) {
    if (typeof item !== "object" || item === null) continue;
    const field: unknown = Reflect.get(item, "field");
    const code: unknown = Reflect.get(item, "code");
    const message: unknown = Reflect.get(item, "message");
    if (typeof field !== "string" || typeof message !== "string") continue;
    // Without a code it's worded as an unknown one: the API's sentence as sent.
    items.push({
      field,
      code: typeof code === "string" ? code : "",
      message,
      params: Reflect.get(item, "params"),
    });
  }
  return problemsByField(items);
}
