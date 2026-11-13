// Checkout rules as the admin edits them: the form's state, its conversion to and from
// the API's rule, the presets "Add rule" offers, and a sentence for each condition.
// Shared by server and client components: what builds text takes the caller's translator
// (and its `Format`, where it writes money or lists).

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { type Currencies, type Format, moneyInput, parseMoney } from "./format";
import { labelOr } from "./keys.ts";
import type { TFor } from "./messages";

type CheckoutT = TFor<"checkout">;

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

/** The fields a condition can read, as the picker lists them. */
const FIELDS = [
  "line1",
  "line2",
  "city",
  "province",
  "postal_code",
  "country",
  "company",
  "first_name",
  "last_name",
  "phone",
  "email",
] as const;

/** What an address can be flagged as: by its format, then by verification. */
const FLAGS = [
  "po_box",
  "military",
  "parcel_locker",
  "non_latin",
  "residential",
  "business",
  "vacant",
] as const;

const VERDICTS = [
  "deliverable",
  "deliverable_with_fixes",
  "missing_subpremise",
  "ambiguous",
  "undeliverable",
  "unknown",
] as const;

type ConditionGroup = "fields" | "address" | "cart";

/** Every kind of condition, grouped as the editor's picker shows them. */
export const CONDITION_TYPES: { value: ConditionType; group: ConditionGroup }[] = [
  { value: "field_missing", group: "fields" },
  { value: "field_present", group: "fields" },
  { value: "field_contains", group: "fields" },
  { value: "field_not_contains", group: "fields" },
  { value: "field_matches", group: "fields" },
  { value: "field_not_matches", group: "fields" },
  { value: "field_longer_than", group: "fields" },
  { value: "field_shorter_than", group: "fields" },
  { value: "postal_code_in", group: "address" },
  { value: "postal_code_not_in", group: "address" },
  { value: "region_in", group: "address" },
  { value: "region_not_in", group: "address" },
  { value: "address_is", group: "address" },
  { value: "address_is_not", group: "address" },
  { value: "verification_is", group: "address" },
  { value: "cart_contains", group: "cart" },
  { value: "cart_not_contains", group: "cart" },
  { value: "subtotal_at_least", group: "cart" },
  { value: "subtotal_below", group: "cart" },
  { value: "weight_at_least", group: "cart" },
  { value: "weight_below", group: "cart" },
];

/** An address field's name; one this admin doesn't know as it is. */
export function fieldLabel(t: CheckoutT, field: string): string {
  const key = `checkout.field_${field}`;
  return labelOr(t, key, field);
}

/** What an address is flagged as, to follow "The address is"; an unknown flag as it is. */
export function flagLabel(t: CheckoutT, flag: string): string {
  const key = `checkout.flag_${flag}`;
  return labelOr(t, key, flag);
}

/** A verdict as it reads inside a sentence ("Verification says …"); an unknown one as it is. */
export function verdictPhrase(t: CheckoutT, verdict: string): string {
  const key = `checkout.verdict_inline_${verdict}`;
  return labelOr(t, key, verdict);
}

/** The fields a condition can read, for the picker. */
export function fieldOptions(t: CheckoutT): Option[] {
  return FIELDS.map((value) => ({ value, label: t(`checkout.field_${value}`) }));
}

/** What an address can be, for the picker. */
export function flagOptions(t: CheckoutT): Option[] {
  return FLAGS.map((value) => ({ value, label: t(`checkout.flag_${value}`) }));
}

/** The verdicts a rule can name, for the picker. */
export function verdictOptions(t: CheckoutT): Option[] {
  return VERDICTS.map((value) => ({ value, label: t(`checkout.verdict_${value}`) }));
}

/** Every kind of condition under its group's name, for the picker. */
export function conditionTypeOptions(
  t: CheckoutT,
): { value: ConditionType; label: string; group: string }[] {
  return CONDITION_TYPES.map(({ value, group }) => ({
    value,
    label: t(`checkout.condition_${value}`),
    group: t(`checkout.condition_group_${group}`),
  }));
}

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
export function pathOptions(t: CheckoutT): Option[] {
  return [
    { value: "", label: t("checkout.path_default") },
    { value: "$", label: t("checkout.path_cart") },
    { value: "$.email", label: t("checkout.path_email") },
    { value: "$.shipping_address", label: t("checkout.path_shipping_address") },
    ...FIELDS.filter((field) => field !== "email").map((field) => ({
      value: `$.shipping_address.${field}`,
      label: t("checkout.path_address_field", { field: t(`checkout.field_${field}`) }),
    })),
    { value: "$.shipping_groups[0]", label: t("checkout.path_shipping_options") },
  ];
}

function label(options: Option[], value: string): string {
  return options.find((o) => o.value === value)?.label ?? value;
}

/** "A, B, C": the names of `ids` among `options`, an ID for one that isn't there. */
function names(format: Format, ids: readonly string[], options: Option[]): string {
  return format.list(
    ids.map((id) => label(options, id)),
    "unit",
  );
}

/** A condition in a sentence: "Phone is empty", "Postal code is 35…, 38…". */
export function conditionSummary(
  t: CheckoutT,
  format: Format,
  c: Condition,
  options: Pick<RuleOptions, "categories">,
): string {
  const field = "field" in c ? fieldLabel(t, c.field) : "";
  const quoted = (values: readonly string[]) =>
    values.map((value) => t("checkout.summary_quoted", { value }));
  switch (c.type) {
    case "field_missing":
      return t("checkout.summary_field_missing", { field });
    case "field_present":
      return t("checkout.summary_field_present", { field });
    case "field_matches":
      return t("checkout.summary_field_matches", { field, pattern: c.pattern });
    case "field_not_matches":
      return t("checkout.summary_field_not_matches", { field, pattern: c.pattern });
    case "field_contains":
      return t("checkout.summary_field_contains", {
        field,
        values: format.list(quoted(c.values), "disjunction"),
      });
    case "field_not_contains":
      return t("checkout.summary_field_not_contains", {
        field,
        values: format.list(quoted(c.values), "unit"),
      });
    case "field_longer_than":
      return t("checkout.summary_field_longer_than", { field, length: c.length });
    case "field_shorter_than":
      return t("checkout.summary_field_shorter_than", { field, length: c.length });
    case "postal_code_in":
    case "postal_code_not_in": {
      const parts = [
        ...(c.prefixes ?? []).map((p) => `${p}…`),
        ...(c.ranges ?? []).map((r) => `${r.from}–${r.to}`),
        ...(c.codes ?? []),
      ];
      const list = format.list(parts.length > 6 ? [...parts.slice(0, 6), "…"] : parts, "unit");
      return c.type === "postal_code_in"
        ? t("checkout.summary_postal_code_in", { list })
        : t("checkout.summary_postal_code_not_in", { list });
    }
    case "region_in":
      return t("checkout.summary_region_in", { list: format.list(c.regions, "unit") });
    case "region_not_in":
      return t("checkout.summary_region_not_in", { list: format.list(c.regions, "unit") });
    case "address_is":
      return t("checkout.summary_address_is", { flag: flagLabel(t, c.flag) });
    case "address_is_not":
      return t("checkout.summary_address_is_not", { flag: flagLabel(t, c.flag) });
    case "verification_is":
      return t("checkout.summary_verification_is", {
        verdicts: format.list(
          c.verdicts.map((v) => verdictPhrase(t, v)),
          "disjunction",
        ),
      });
    case "cart_contains":
    case "cart_not_contains": {
      const products = (c.products ?? []).length;
      const list = format.list(
        [
          ...(products > 0 ? [t("checkout.summary_chosen_products", { count: products })] : []),
          ...(c.categories ?? []).map((id) => label(options.categories, id)),
        ],
        "disjunction",
      );
      return c.type === "cart_contains"
        ? t("checkout.summary_cart_contains", { list })
        : t("checkout.summary_cart_not_contains", { list });
    }
    case "subtotal_at_least":
      return t("checkout.summary_subtotal_at_least", {
        amounts: c.amounts.map((m) => format.money(m)).join(" / "),
      });
    case "subtotal_below":
      return t("checkout.summary_subtotal_below", {
        amounts: c.amounts.map((m) => format.money(m)).join(" / "),
      });
    case "weight_at_least":
      return t("checkout.summary_weight_at_least", { grams: String(c.grams) });
    case "weight_below":
      return t("checkout.summary_weight_below", { grams: String(c.grams) });
  }
}

/** A rule's conditions in one sentence: "A and B and C", since each may hold a list. */
export function conditionsSummary(
  t: CheckoutT,
  format: Format,
  conditions: readonly Condition[],
  options: Pick<RuleOptions, "categories">,
): string {
  const [first = "", ...rest] = conditions.map((c) => conditionSummary(t, format, c, options));
  return rest.reduce((so, next) => t("checkout.summary_both", { first: so, second: next }), first);
}

/** Where a rule applies, in a sentence; empty when everywhere. */
export function appliesSummary(
  t: CheckoutT,
  format: Format,
  rule: Rule,
  options: RuleOptions,
): string {
  const to = rule.applies_to;
  const parts: string[] = [];
  const countries = to.countries ?? { mode: "all" };
  if (countries.mode !== "all" && "codes" in countries) {
    const list = names(format, countries.codes, options.countries);
    parts.push(
      countries.mode === "include"
        ? t("checkout.applies_only", { list })
        : t("checkout.applies_not", { list }),
    );
  }
  if ((to.markets ?? []).length > 0) parts.push(names(format, to.markets ?? [], options.markets));
  if ((to.channels ?? []).length > 0) {
    parts.push(names(format, to.channels ?? [], options.channels));
  }
  if ((to.shipping_options ?? []).length > 0) {
    parts.push(
      t("checkout.applies_with", {
        list: names(format, to.shipping_options ?? [], options.shippingOptions),
      }),
    );
  }
  return parts.join(" · ");
}

/** A preset's stable key: `checkout.preset_<key>_title` and `…_description` name it. */
export type PresetKey =
  | "no_po_boxes"
  | "no_military"
  | "house_number"
  | "phone_required"
  | "line_length"
  | "canary_islands"
  | "undeliverable";

/**
 * A preset "Add rule" offers: a filled-in rule the merchant then edits. The rule's name
 * and the message shoppers read are stored content, so they stay English whatever the
 * staff member's language: the merchant rewrites them in the store's.
 */
export type Preset = {
  key: PresetKey;
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
