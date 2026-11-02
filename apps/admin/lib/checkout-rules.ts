// Checkout rules as the admin edits them: the form's state, its conversion to and from
// the API's rule, the presets "Add rule" offers, and a sentence for each condition.
// Shared by server and client components.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { type Currencies, formatMoney, moneyInput, parseMoney } from "./format";

export type Rule = AdminSchemas["CheckoutRuleResponse"];
export type Condition = AdminSchemas["RuleCondition"];
export type ConditionType = Condition["type"];
export type RuleBody = AdminSchemas["CreateCheckoutRuleRequest"];
export type TestResult = AdminSchemas["TestCheckoutRulesResponse"];

/** A choice in a picker. */
export type Option = { value: string; label: string };

/** What the editor's pickers offer. A list the role can't read is empty. */
export type RuleOptions = {
  countries: Option[];
  /** The first market's first country, where "Try an address" starts. */
  defaultCountry?: string;
  markets: (Option & { currency: string })[];
  channels: Option[];
  /** Every zone's options, labelled "Zone › Option". */
  shippingOptions: Option[];
  categories: Option[];
};

export const FIELDS: Option[] = [
  { value: "line1", label: "Address line 1" },
  { value: "line2", label: "Address line 2" },
  { value: "city", label: "City" },
  { value: "province", label: "Region" },
  { value: "postal_code", label: "Postal code" },
  { value: "country", label: "Country" },
  { value: "company", label: "Company" },
  { value: "first_name", label: "First name" },
  { value: "last_name", label: "Last name" },
  { value: "phone", label: "Phone" },
  { value: "email", label: "Email" },
];

export const FLAGS: Option[] = [
  { value: "po_box", label: "a PO box" },
  { value: "military", label: "a military address (APO, FPO, DPO)" },
  { value: "parcel_locker", label: "a parcel locker (Packstation)" },
  { value: "non_latin", label: "written in a non-Latin script" },
  { value: "residential", label: "residential (verification)" },
  { value: "business", label: "a business (verification)" },
  { value: "vacant", label: "vacant (verification)" },
];

export const VERDICTS: Option[] = [
  { value: "deliverable", label: "Deliverable" },
  { value: "deliverable_with_fixes", label: "Deliverable with fixes" },
  { value: "missing_subpremise", label: "Missing an apartment or unit" },
  { value: "ambiguous", label: "Ambiguous" },
  { value: "undeliverable", label: "Undeliverable" },
  { value: "unknown", label: "Unknown (verification failed)" },
];

/** Every kind of condition, grouped as the editor's picker shows them. */
export const CONDITION_TYPES: { value: ConditionType; label: string; group: string }[] = [
  { value: "field_missing", label: "A field is empty", group: "Fields" },
  { value: "field_present", label: "A field isn't empty", group: "Fields" },
  { value: "field_contains", label: "A field contains", group: "Fields" },
  { value: "field_not_contains", label: "A field doesn't contain", group: "Fields" },
  { value: "field_matches", label: "A field matches a pattern", group: "Fields" },
  { value: "field_not_matches", label: "A field doesn't match a pattern", group: "Fields" },
  { value: "field_longer_than", label: "A field is longer than", group: "Fields" },
  { value: "field_shorter_than", label: "A field is shorter than", group: "Fields" },
  { value: "postal_code_in", label: "The postal code is in", group: "Address" },
  { value: "postal_code_not_in", label: "The postal code isn't in", group: "Address" },
  { value: "region_in", label: "The region is one of", group: "Address" },
  { value: "region_not_in", label: "The region isn't one of", group: "Address" },
  { value: "address_is", label: "The address is", group: "Address" },
  { value: "address_is_not", label: "The address isn't", group: "Address" },
  { value: "verification_is", label: "Verification says", group: "Address" },
  { value: "cart_contains", label: "The cart contains", group: "Cart" },
  { value: "cart_not_contains", label: "The cart doesn't contain", group: "Cart" },
  { value: "subtotal_at_least", label: "The subtotal is at least", group: "Cart" },
  { value: "subtotal_below", label: "The subtotal is below", group: "Cart" },
  { value: "weight_at_least", label: "The weight is at least", group: "Cart" },
  { value: "weight_below", label: "The weight is below", group: "Cart" },
];

/**
 * One condition as the form edits it: every kind's fields at once, lists as text (one
 * entry per line or comma), so typing never fights the conversion.
 */
export type ConditionForm = {
  /** The form's own handle on it, for React's keys: conditions have no ID. */
  key: string;
  type: ConditionType;
  field: string;
  pattern: string;
  caseInsensitive: boolean;
  values: string;
  length: string;
  prefixes: string;
  /** `35000-35999`, one per line. */
  ranges: string;
  codes: string;
  regions: string;
  flag: string;
  verdicts: string[];
  products: string[];
  categories: string[];
  /** Major units by currency. */
  amounts: Record<string, string>;
  grams: string;
};

export type RuleForm = {
  name: string;
  enabled: boolean;
  countriesMode: "all" | "include" | "exclude";
  countries: string[];
  markets: string[];
  channels: string[];
  shippingOptions: string[];
  conditions: ConditionForm[];
  severity: "error" | "warning";
  message: string;
  /** Empty: where the rule points by default. */
  path: string;
  code: string;
};

let nextKey = 0;

export function blankCondition(type: ConditionType = "field_missing"): ConditionForm {
  nextKey += 1;
  return {
    key: `condition-${nextKey}`,
    type,
    field: type === "field_missing" ? "phone" : "line1",
    pattern: "",
    caseInsensitive: true,
    values: "",
    length: "35",
    prefixes: "",
    ranges: "",
    codes: "",
    regions: "",
    flag: "po_box",
    verdicts: ["undeliverable"],
    products: [],
    categories: [],
    amounts: {},
    grams: "",
  };
}

export function blankRule(): RuleForm {
  return {
    name: "",
    enabled: true,
    countriesMode: "all",
    countries: [],
    markets: [],
    channels: [],
    shippingOptions: [],
    conditions: [blankCondition()],
    severity: "error",
    message: "",
    path: "",
    code: "",
  };
}

/** Entries of a list typed one per line or separated by commas. */
export function entries(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter((s) => s !== "");
}

function lines(list: readonly string[]): string {
  return list.join("\n");
}

/** A form condition as the API takes it. */
export function toCondition(c: ConditionForm, currencies: Currencies): Condition {
  const postal = {
    prefixes: entries(c.prefixes),
    codes: entries(c.codes),
    ranges: entries(c.ranges).map((r) => {
      const [from = "", to = from] = r.split(/\s*(?:–|-|\.\.)\s*(?=\S)/u);
      return { from, to };
    }),
  };
  const amounts = Object.entries(c.amounts)
    .filter(([, text]) => text.trim() !== "")
    .map(([currency, text]) => ({
      currency,
      amount: parseMoney(text, currency, currencies) ?? Number.NaN,
    }));
  switch (c.type) {
    case "field_missing":
    case "field_present":
      return { type: c.type, field: c.field };
    case "field_matches":
    case "field_not_matches":
      return {
        type: c.type,
        field: c.field,
        pattern: c.pattern,
        case_insensitive: c.caseInsensitive,
      };
    case "field_contains":
    case "field_not_contains":
      return { type: c.type, field: c.field, values: entries(c.values) };
    case "field_longer_than":
    case "field_shorter_than":
      return { type: c.type, field: c.field, length: Number.parseInt(c.length, 10) || 0 };
    case "postal_code_in":
    case "postal_code_not_in":
      return { type: c.type, ...postal };
    case "region_in":
    case "region_not_in":
      return { type: c.type, regions: entries(c.regions) };
    case "address_is":
    case "address_is_not":
      return { type: c.type, flag: c.flag };
    case "verification_is":
      return { type: c.type, verdicts: c.verdicts };
    case "cart_contains":
    case "cart_not_contains":
      return { type: c.type, products: c.products, categories: c.categories };
    case "subtotal_at_least":
    case "subtotal_below":
      return { type: c.type, amounts };
    case "weight_at_least":
    case "weight_below":
      return { type: c.type, grams: Number.parseInt(c.grams, 10) || 0 };
  }
}

/** An API condition as the form edits it. */
export function fromCondition(c: Condition, currencies: Currencies): ConditionForm {
  const form = blankCondition(c.type);
  if ("field" in c) form.field = c.field;
  if ("pattern" in c) {
    form.pattern = c.pattern;
    form.caseInsensitive = c.case_insensitive ?? false;
  }
  if ("values" in c) form.values = lines(c.values);
  if ("length" in c) form.length = String(c.length);
  if ("prefixes" in c) form.prefixes = lines(c.prefixes ?? []);
  if ("codes" in c) form.codes = lines(c.codes ?? []);
  if ("ranges" in c) form.ranges = lines((c.ranges ?? []).map((r) => `${r.from}-${r.to}`));
  if ("regions" in c) form.regions = lines(c.regions);
  if ("flag" in c) form.flag = c.flag;
  if ("verdicts" in c) form.verdicts = c.verdicts;
  if ("products" in c) form.products = c.products ?? [];
  if ("categories" in c) form.categories = c.categories ?? [];
  if ("amounts" in c) {
    form.amounts = Object.fromEntries(
      c.amounts.map((m) => [m.currency, moneyInput(m, currencies)]),
    );
  }
  if ("grams" in c) form.grams = String(c.grams);
  return form;
}

/** The form as the API's rule body. */
export function toBody(form: RuleForm, currencies: Currencies): RuleBody {
  return {
    name: form.name.trim(),
    enabled: form.enabled,
    applies_to: {
      countries:
        form.countriesMode === "all"
          ? { mode: "all" }
          : { mode: form.countriesMode, codes: form.countries },
      markets: form.markets,
      channels: form.channels,
      shipping_options: form.shippingOptions,
    },
    conditions: form.conditions.map((c) => toCondition(c, currencies)),
    outcome: {
      severity: form.severity,
      message: form.message,
      path: form.path === "" ? null : form.path,
      code: form.code.trim() === "" ? null : form.code.trim(),
    },
  };
}

export function formOf(rule: Rule, currencies: Currencies): RuleForm {
  const countries = rule.applies_to.countries ?? { mode: "all" };
  return {
    name: rule.name,
    enabled: rule.enabled,
    countriesMode: countries.mode,
    countries: "codes" in countries ? countries.codes : [],
    markets: rule.applies_to.markets ?? [],
    channels: rule.applies_to.channels ?? [],
    shippingOptions: rule.applies_to.shipping_options ?? [],
    conditions: rule.conditions.map((c) => fromCondition(c, currencies)),
    severity: rule.outcome.severity === "warning" ? "warning" : "error",
    message: rule.outcome.message,
    path: rule.outcome.path ?? "",
    code: rule.outcome.code ?? "",
  };
}

/** Where a message may point: the cart's paths a rule can name. */
export const PATHS: Option[] = [
  { value: "", label: "Where the rule is about (default)" },
  { value: "$", label: "The whole cart (top of checkout)" },
  { value: "$.email", label: "Email" },
  { value: "$.shipping_address", label: "The shipping address" },
  ...FIELDS.filter((f) => f.value !== "email").map((f) => ({
    value: `$.shipping_address.${f.value}`,
    label: `Address: ${f.label}`,
  })),
  { value: "$.shipping_groups[0]", label: "The shipping options" },
];

function label(options: Option[], value: string): string {
  return options.find((o) => o.value === value)?.label ?? value;
}

function names(ids: readonly string[], options: Option[]): string {
  return ids.map((id) => label(options, id)).join(", ");
}

/** A condition in a sentence: "Phone is empty", "Postal code starts with 35, 38". */
export function conditionSummary(
  c: Condition,
  options: Pick<RuleOptions, "categories">,
  currencies: Currencies,
): string {
  const field = "field" in c ? label(FIELDS, c.field) : "";
  switch (c.type) {
    case "field_missing":
      return `${field} is empty`;
    case "field_present":
      return `${field} isn't empty`;
    case "field_matches":
      return `${field} matches /${c.pattern}/`;
    case "field_not_matches":
      return `${field} doesn't match /${c.pattern}/`;
    case "field_contains":
      return `${field} contains ${c.values.map((v) => `“${v}”`).join(" or ")}`;
    case "field_not_contains":
      return `${field} contains none of ${c.values.map((v) => `“${v}”`).join(", ")}`;
    case "field_longer_than":
      return `${field} is longer than ${c.length} characters`;
    case "field_shorter_than":
      return `${field} is shorter than ${c.length} characters`;
    case "postal_code_in":
    case "postal_code_not_in": {
      const parts = [
        ...(c.prefixes ?? []).map((p) => `${p}…`),
        ...(c.ranges ?? []).map((r) => `${r.from}–${r.to}`),
        ...(c.codes ?? []),
      ];
      const list = parts.length > 6 ? `${parts.slice(0, 6).join(", ")}, …` : parts.join(", ");
      return `Postal code ${c.type === "postal_code_in" ? "is" : "isn't"} ${list}`;
    }
    case "region_in":
      return `Region is ${c.regions.join(", ")}`;
    case "region_not_in":
      return `Region isn't ${c.regions.join(", ")}`;
    case "address_is":
      return `The address is ${label(FLAGS, c.flag)}`;
    case "address_is_not":
      return `The address isn't ${label(FLAGS, c.flag)}`;
    case "verification_is":
      return `Verification says ${c.verdicts.map((v) => label(VERDICTS, v).toLowerCase()).join(" or ")}`;
    case "cart_contains":
    case "cart_not_contains": {
      const products = (c.products ?? []).length;
      const listed = [
        ...(products > 0 ? [`${products} chosen product${products === 1 ? "" : "s"}`] : []),
        ...(c.categories ?? []).map((id) => label(options.categories, id)),
      ].join(" or ");
      return `The cart ${c.type === "cart_contains" ? "contains" : "doesn't contain"} ${listed}`;
    }
    case "subtotal_at_least":
      return `Subtotal is at least ${c.amounts.map((m) => formatMoney(m, currencies)).join(" / ")}`;
    case "subtotal_below":
      return `Subtotal is below ${c.amounts.map((m) => formatMoney(m, currencies)).join(" / ")}`;
    case "weight_at_least":
      return `What's shipped weighs at least ${c.grams} g`;
    case "weight_below":
      return `What's shipped weighs less than ${c.grams} g`;
  }
}

/** Where a rule applies, in a sentence; empty when everywhere. */
export function appliesSummary(rule: Rule, options: RuleOptions): string {
  const to = rule.applies_to;
  const parts: string[] = [];
  const countries = to.countries ?? { mode: "all" };
  if (countries.mode !== "all" && "codes" in countries) {
    const list = names(countries.codes, options.countries);
    parts.push(countries.mode === "include" ? `Only ${list}` : `Not ${list}`);
  }
  if ((to.markets ?? []).length > 0) parts.push(names(to.markets ?? [], options.markets));
  if ((to.channels ?? []).length > 0) parts.push(names(to.channels ?? [], options.channels));
  if ((to.shipping_options ?? []).length > 0) {
    parts.push(`with ${names(to.shipping_options ?? [], options.shippingOptions)}`);
  }
  return parts.join(" · ");
}

/** A preset "Add rule" offers: a filled-in rule the merchant then edits. */
export type Preset = {
  key: string;
  title: string;
  description: string;
  rule: () => RuleForm;
};

const rule = (patch: Partial<RuleForm>): RuleForm => ({ ...blankRule(), ...patch });

const condition = (type: ConditionType, patch: Partial<ConditionForm>): ConditionForm => ({
  ...blankCondition(type),
  ...patch,
});

export const PRESETS: Preset[] = [
  {
    key: "no_po_boxes",
    title: "No PO boxes",
    description: "For every option, or only couriers that can't deliver to them.",
    rule: () =>
      rule({
        name: "No PO boxes",
        conditions: [condition("address_is", { flag: "po_box" })],
        message: "We can't deliver to PO boxes. Enter a street address.",
        code: "po_box_not_allowed",
      }),
  },
  {
    key: "no_military",
    title: "No military addresses",
    description: "APO, FPO and DPO addresses, which only the postal service delivers to.",
    rule: () =>
      rule({
        name: "No military addresses",
        conditions: [condition("address_is", { flag: "military" })],
        message: "We can't ship to APO, FPO or DPO addresses.",
        code: "military_address_not_allowed",
      }),
  },
  {
    key: "house_number",
    title: "House number required",
    description: "The first address line must have a number (the Netherlands, Germany, Belgium).",
    rule: () =>
      rule({
        name: "House number required",
        countriesMode: "include",
        countries: ["NL", "DE", "BE"],
        conditions: [condition("field_not_matches", { field: "line1", pattern: "\\d" })],
        message: "Add your house number to the first address line.",
        code: "house_number_required",
      }),
  },
  {
    key: "phone_required",
    title: "Phone required",
    description: "For the shipping options whose carrier calls ahead.",
    rule: () =>
      rule({
        name: "Phone required",
        conditions: [condition("field_missing", { field: "phone" })],
        message: "Add a phone number: the courier needs it to deliver.",
        code: "phone_required",
      }),
  },
  {
    key: "line_length",
    title: "Lines of at most 35 characters",
    description: "What UPS and FedEx print on a label.",
    rule: () =>
      rule({
        name: "Address lines of at most 35 characters",
        conditions: [condition("field_longer_than", { field: "line1", length: "35" })],
        message: "Keep the address line to 35 characters: the label cuts off the rest.",
        code: "line_too_long",
      }),
  },
  {
    key: "canary_islands",
    title: "No Canary Islands, Ceuta or Melilla",
    description: "Spanish postal codes outside the EU's VAT area.",
    rule: () =>
      rule({
        name: "No Canary Islands, Ceuta or Melilla",
        countriesMode: "include",
        countries: ["ES"],
        conditions: [condition("postal_code_in", { prefixes: "35\n38\n51\n52" })],
        message: "We can't ship to the Canary Islands, Ceuta or Melilla.",
        code: "region_not_served",
      }),
  },
  {
    key: "undeliverable",
    title: "Refuse undeliverable addresses",
    description: "When the verification app finds no such address.",
    rule: () =>
      rule({
        name: "Refuse undeliverable addresses",
        conditions: [condition("verification_is", { verdicts: ["undeliverable"] })],
        message: "We couldn't find this address. Check it and try again.",
        path: "$.shipping_address",
        code: "address_undeliverable",
      }),
  },
];

export function presetOf(key: string | undefined): Preset | undefined {
  return PRESETS.find((p) => p.key === key);
}

/** The currencies the store's markets sell in, for subtotal amounts. */
export function marketCurrencies(options: RuleOptions): string[] {
  return [...new Set(options.markets.map((m) => m.currency))].toSorted();
}

export { formatMoney };
