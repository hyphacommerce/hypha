// Price lists as the admin's screens use them: labels for kinds and phases, the
// adjustment's percentage and what it covers, the schedule in the store's time zone,
// the settings form and the requests it makes, and the fixed-price errors by row. Plain
// functions and types only, shared by server and client code (and run by `node --test`):
// what they say comes from the caller's translator (`t`) and `Format`.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { type CategoryNode, subtree } from "./categories.ts";
import type { Format } from "./format.ts";
import type { TFor } from "./messages";
import { adjustAmount, percentOf } from "./price-grid/money.ts";
import { MAX_BREAKS, type Money, type PriceT } from "./price-grid/types.ts";

export type PriceList = AdminSchemas["PriceListResponse"];
export type AppliesTo = AdminSchemas["AppliesToDto"];
export type Adjustment = AdminSchemas["AdjustmentDto"];
export type CreatePriceList = AdminSchemas["CreatePriceListRequest"];
export type UpdatePriceList = AdminSchemas["UpdatePriceListRequest"];

export type Kind = "sale" | "override";
export type Status = "draft" | "active";
export type Phase = "draft" | "scheduled" | "active" | "expired";
export type CompareAtMode = "adjusted" | "hidden";
export type Scope = "all" | "products" | "categories";

/** The most products or categories an adjustment may name. */
export const MAX_TARGETS = 1000;
/** The most price lists a catalog holds. */
export const MAX_LISTS_PER_CATALOG = 50;
/** The most customer groups an audience names. */
export const MAX_AUDIENCE_GROUPS = 100;

/**
 * The kinds, in the order offered. Each has a label and a description:
 * `price_lists.kind_<kind>` and `price_lists.kind_<kind>_description`.
 */
export const KINDS: readonly Kind[] = ["sale", "override"];

/** A kind as the admin says it (an unknown one as it is). */
export function kindLabel(t: PriceT, kind: string): string {
  const known = KINDS.find((k) => k === kind);
  return known ? t(`price_lists.kind_${known}`) : kind;
}

/** The phases, in the order a filter offers them. */
const PHASE_ORDER: readonly Phase[] = ["active", "scheduled", "expired", "draft"];

/**
 * A translator for the phases' names: any at all, since they're `common`'s (a price list
 * and a discount are scheduled the same way, and say so with the same words).
 */
export type PhaseT = TFor<"common">;

/** A filter's options for the phases, in `t`'s language. */
export function phaseOptions(t: PhaseT): { value: Phase; label: string }[] {
  return PHASE_ORDER.map((value) => ({ value, label: t(`common.phase_${value}`) }));
}

const PHASE_VARIANTS = {
  active: "success",
  scheduled: "info",
  expired: "warning",
  draft: "neutral",
} as const satisfies Record<Phase, string>;

/** A phase's badge: its label, in `t`'s language, and its colour. An unknown phase as it is. */
export function phaseBadge(
  phase: string,
  t: PhaseT,
): {
  label: string;
  variant: "neutral" | "info" | "success" | "warning";
} {
  const known = PHASE_ORDER.find((p) => p === phase);
  if (!known) return { label: phase, variant: "neutral" };
  return { label: t(`common.phase_${known}`), variant: PHASE_VARIANTS[known] };
}

// ---- the adjustment ----

/** The percent input's text for `bps`: "-20" for -2000, "5.5" for 550. */
export function percentInput(bps: number): string {
  return String(bps / 100);
}

/** "−20%" from -2000, always signed, as `format`'s locale writes a percentage. */
export function percentLabel(format: Format, bps: number): string {
  // A real minus sign where the locale writes a hyphen.
  return format.percent(bps / 10_000, { signDisplay: "always" }).replace("-", "−");
}

/** "−20% on every product", "−10% on 3 categories", "No adjustment". */
export function adjustmentLabel(
  t: PriceT,
  format: Format,
  adjustment: Adjustment | null | undefined,
): string {
  if (!adjustment) return t("price_lists.no_adjustment");
  const percent = percentLabel(format, adjustment.percent_bps);
  const to = adjustment.applies_to ?? { type: "all" };
  switch (to.type) {
    case "products":
      return t("price_lists.adjustment_products", { percent, count: to.products.length });
    case "categories":
      return t("price_lists.adjustment_categories", { percent, count: to.categories.length });
    default:
      return t("price_lists.adjustment_all", { percent });
  }
}

/**
 * The categories an adjustment's `applies_to` covers, each with every category below
 * it (a product in a subcategory counts); `null` when it isn't about categories.
 */
export function coveredCategories(
  appliesTo: AppliesTo | undefined,
  tree: CategoryNode[],
): Set<string> | null {
  if (appliesTo?.type !== "categories") return null;
  const covered = new Set<string>();
  for (const id of appliesTo.categories) {
    covered.add(id);
    for (const below of subtree(tree, id)) covered.add(below);
  }
  return covered;
}

/**
 * Whether the adjustment prices `product`: every product, the products it names, or a
 * product directly in one of the `covered` categories (`coveredCategories`).
 */
export function coversProduct(
  appliesTo: AppliesTo | undefined,
  product: { id: string; categories: readonly string[] },
  covered: Set<string> | null,
): boolean {
  const to = appliesTo ?? { type: "all" };
  switch (to.type) {
    case "products":
      return to.products.includes(product.id);
    case "categories":
      return product.categories.some((c) => covered?.has(c) ?? to.categories.includes(c));
    default:
      return true;
  }
}

/**
 * What the list's adjustment makes of `base` for a variant of `product`, or null when
 * the list has no adjustment, it doesn't cover the product, or the base price is in
 * another currency (the API can't convert it, so the list can't adjust it).
 */
export function adjustedPrice(
  list: { currency: string; adjustment?: Adjustment | null },
  product: { id: string; categories: readonly string[] },
  base: Money,
  covered: Set<string> | null,
): Money | null {
  const adjustment = list.adjustment;
  if (!adjustment || base.currency !== list.currency) return null;
  if (!coversProduct(adjustment.applies_to, product, covered)) return null;
  const amount = adjustAmount(base.amount, adjustment.percent_bps);
  return amount === null ? null : { amount, currency: base.currency };
}

// ---- the schedule, in the store's time zone ----

// The wall clock in a time zone, read from `Intl`'s parts: arithmetic, never shown, so
// its locale is fixed whatever the reader's language.
const CLOCK_LOCALE = "en-US";

/** How far `timeZone`'s clock is ahead of UTC at instant `at`, in milliseconds. */
function zoneOffset(at: number, timeZone: string): number {
  const parts = new Intl.DateTimeFormat(CLOCK_LOCALE, {
    timeZone,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).formatToParts(new Date(at));
  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value);
  const local = Date.UTC(
    get("year"),
    get("month") - 1,
    get("day"),
    get("hour"),
    get("minute"),
    get("second"),
  );
  return local - (at - (at % 1000));
}

/**
 * The instant a wall-clock time in `timeZone` names (`2026-11-27T00:00`, as the date
 * and time input gives it), as RFC 3339 in UTC; null when it isn't one.
 */
export function fromLocalInput(local: string, timeZone: string): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(local.trim());
  if (!match) return null;
  const [, y, mo, d, h, mi, s = "0"] = match;
  const guess = Date.UTC(Number(y), Number(mo) - 1, Number(d), Number(h), Number(mi), Number(s));
  if (Number.isNaN(guess)) return null;
  let at = guess - zoneOffset(guess, timeZone);
  // Once more, for a time on a day the offset changes (daylight saving).
  at = guess - zoneOffset(at, timeZone);
  return new Date(at).toISOString().replace(".000Z", "Z");
}

/** An instant (RFC 3339) as the wall-clock time in `timeZone` the input shows. */
export function toLocalInput(rfc3339: string | null | undefined, timeZone: string): string {
  if (!rfc3339) return "";
  const at = new Date(rfc3339);
  if (Number.isNaN(at.getTime())) return "";
  const parts = new Intl.DateTimeFormat(CLOCK_LOCALE, {
    timeZone,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).formatToParts(at);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")}T${get("hour")}:${get("minute")}`;
}

// ---- the settings form ----

export type ListForm = {
  name: string;
  description: string;
  currency: string;
  kind: Kind;
  status: Status;
  /** Wall-clock times in the store's zone (`2026-11-27T00:00`); "" for none. */
  startsAt: string;
  endsAt: string;
  compareAt: CompareAtMode;
  hasAdjustment: boolean;
  /** Signed percent as typed: "-20" is 20% off. */
  percent: string;
  scope: Scope;
  products: string[];
  categories: string[];
  /** The customer groups it prices for; none is everyone. */
  groups: string[];
};

export type FormField =
  | "name"
  | "description"
  | "currency"
  | "startsAt"
  | "endsAt"
  | "percent"
  | "products"
  | "categories"
  | "groups";

export type FormErrors = Partial<Record<FormField, string>>;

const FORM_FIELDS: readonly FormField[] = [
  "name",
  "description",
  "currency",
  "startsAt",
  "endsAt",
  "percent",
  "products",
  "categories",
  "groups",
];

/** `errors` without those of the fields `change` sets: they're being fixed. */
export function clearErrors(errors: FormErrors, change: Partial<ListForm>): FormErrors {
  const next = { ...errors };
  for (const field of FORM_FIELDS) if (field in change) delete next[field];
  return next;
}

/** A new list's form: an override list, active, at once, no adjustment. */
export function emptyForm(currency: string): ListForm {
  return {
    name: "",
    description: "",
    currency,
    kind: "override",
    status: "active",
    startsAt: "",
    endsAt: "",
    compareAt: "adjusted",
    hasAdjustment: false,
    percent: "",
    scope: "all",
    products: [],
    categories: [],
    groups: [],
  };
}

export function formFromList(list: PriceList, timeZone: string): ListForm {
  const to = list.adjustment?.applies_to ?? { type: "all" as const };
  return {
    name: list.name,
    description: list.description ?? "",
    currency: list.currency,
    kind: list.kind === "sale" ? "sale" : "override",
    status: list.status === "draft" ? "draft" : "active",
    startsAt: toLocalInput(list.starts_at, timeZone),
    endsAt: toLocalInput(list.ends_at, timeZone),
    compareAt: list.compare_at === "hidden" ? "hidden" : "adjusted",
    hasAdjustment: Boolean(list.adjustment),
    percent: list.adjustment ? percentInput(list.adjustment.percent_bps) : "",
    scope: to.type,
    products: to.type === "products" ? to.products : [],
    categories: to.type === "categories" ? to.categories : [],
    groups: list.audience?.groups ?? [],
  };
}

/** "Everyone", "Wholesale", "Wholesale and VIP", "3 customer groups": who a list prices
 * for, by the groups' names (an ID where the name isn't known). */
export function audienceLabel(
  t: PriceT,
  format: Format,
  audience: { groups: readonly string[] } | null | undefined,
  names: Record<string, string>,
): string {
  const groups = audience?.groups ?? [];
  if (groups.length === 0) return t("price_lists.audience_everyone");
  if (groups.length > 2) return t("price_lists.audience_groups_count", { count: groups.length });
  return format.list(groups.map((g) => names[g] ?? g));
}

export type TemplateKey = "sale" | "clearance" | "market" | "wholesale";

/**
 * The new list page's starting points: each sets the fields its kind of list needs. A
 * template's name and description are `price_lists.template_<key>` and
 * `price_lists.template_<key>_description`.
 */
export const TEMPLATES: {
  key: TemplateKey;
  apply: (form: ListForm, now: { startsAt: string; endsAt: string }) => ListForm;
}[] = [
  {
    key: "sale",
    apply: (form, now) => ({
      ...form,
      kind: "sale",
      hasAdjustment: true,
      percent: "-20",
      scope: "categories",
      startsAt: now.startsAt,
      endsAt: now.endsAt,
    }),
  },
  {
    key: "clearance",
    apply: (form) => ({
      ...form,
      kind: "sale",
      hasAdjustment: true,
      percent: "-30",
      scope: "categories",
      startsAt: "",
      endsAt: "",
    }),
  },
  {
    key: "market",
    apply: (form) => ({
      ...form,
      kind: "override",
      hasAdjustment: true,
      percent: "0",
      scope: "all",
      startsAt: "",
      endsAt: "",
    }),
  },
  {
    key: "wholesale",
    apply: (form) => ({
      ...form,
      kind: "override",
      hasAdjustment: false,
      percent: "",
      scope: "all",
      startsAt: "",
      endsAt: "",
    }),
  },
];

/** The most characters a name or a description takes. */
const MAX_TEXT = 255;

/** What's wrong with the form, by field, in `t`'s language; empty when it can be sent. */
export function checkForm(t: PriceT, form: ListForm, timeZone: string): FormErrors {
  const errors: FormErrors = {};
  const tooLong = t("price_lists.form_too_long", { max: MAX_TEXT });
  if (form.name.trim() === "") errors.name = t("price_lists.form_name_required");
  else if (form.name.trim().length > MAX_TEXT) errors.name = tooLong;
  if (form.description.trim().length > MAX_TEXT) errors.description = tooLong;
  if (form.currency === "") errors.currency = t("price_lists.form_currency_required");
  const starts = form.startsAt ? fromLocalInput(form.startsAt, timeZone) : null;
  const ends = form.endsAt ? fromLocalInput(form.endsAt, timeZone) : null;
  if (form.startsAt && !starts) errors.startsAt = t("price_lists.form_date_time_invalid");
  if (form.endsAt && !ends) errors.endsAt = t("price_lists.form_date_time_invalid");
  if (starts && ends && Date.parse(ends) <= Date.parse(starts)) {
    errors.endsAt = t("price_lists.form_end_before_start");
  }
  if (form.groups.length > MAX_AUDIENCE_GROUPS) {
    errors.groups = t("price_lists.form_too_many_groups", { max: MAX_AUDIENCE_GROUPS });
  }
  if (form.hasAdjustment) {
    if (percentOf(form.percent) === null) {
      errors.percent = t("price_lists.form_percent_invalid");
    }
    if (form.scope === "products" && form.products.length === 0) {
      errors.products = t("price_lists.form_products_required");
    } else if (form.products.length > MAX_TARGETS) {
      errors.products = t("price_lists.form_too_many_products", { max: MAX_TARGETS });
    }
    if (form.scope === "categories" && form.categories.length === 0) {
      errors.categories = t("price_lists.form_categories_required");
    } else if (form.categories.length > MAX_TARGETS) {
      errors.categories = t("price_lists.form_too_many_categories", { max: MAX_TARGETS });
    }
  }
  return errors;
}

function adjustmentOf(form: ListForm): Adjustment | null {
  if (!form.hasAdjustment) return null;
  const percent_bps = percentOf(form.percent) ?? 0;
  const applies_to: AppliesTo =
    form.scope === "products"
      ? { type: "products", products: form.products }
      : form.scope === "categories"
        ? { type: "categories", categories: form.categories }
        : { type: "all" };
  return { percent_bps, applies_to };
}

function sameAdjustment(a: Adjustment | null | undefined, b: Adjustment | null): boolean {
  if (!a || !b) return !a && !b;
  const key = (x: Adjustment) => {
    const to = x.applies_to ?? { type: "all" };
    const ids =
      to.type === "products"
        ? to.products.toSorted()
        : to.type === "categories"
          ? to.categories.toSorted()
          : [];
    return JSON.stringify([x.percent_bps, to.type, ids]);
  };
  return key(a) === key(b);
}

/** `POST /admin/price-lists`'s body for the form, or what to fix first. */
export function createRequest(
  t: PriceT,
  form: ListForm,
  timeZone: string,
): { body: CreatePriceList } | { errors: FormErrors } {
  const errors = checkForm(t, form, timeZone);
  if (Object.keys(errors).length > 0) return { errors };
  const body: CreatePriceList = {
    name: form.name.trim(),
    currency: form.currency,
    kind: form.kind,
    status: form.status,
  };
  if (form.description.trim()) body.description = form.description.trim();
  if (form.startsAt) body.starts_at = fromLocalInput(form.startsAt, timeZone);
  if (form.endsAt) body.ends_at = fromLocalInput(form.endsAt, timeZone);
  // The compare-at mode only means something for regular prices.
  if (form.kind === "override") body.compare_at = form.compareAt;
  const adjustment = adjustmentOf(form);
  if (adjustment) body.adjustment = adjustment;
  if (form.groups.length > 0) body.audience = { groups: form.groups };
  return { body };
}

/**
 * `PATCH /admin/price-lists/{id}`'s body: only what differs from `list` (JSON Merge
 * Patch: `null` clears), or what to fix first. An empty body means nothing changed.
 */
export function updateRequest(
  t: PriceT,
  form: ListForm,
  list: PriceList,
  timeZone: string,
): { body: UpdatePriceList } | { errors: FormErrors } {
  const errors = checkForm(t, form, timeZone);
  if (Object.keys(errors).length > 0) return { errors };
  const saved = formFromList(list, timeZone);
  const body: UpdatePriceList = {};
  if (form.name.trim() !== list.name) body.name = form.name.trim();
  if (form.description.trim() !== (list.description ?? "")) {
    body.description = form.description.trim() || null;
  }
  if (form.kind !== saved.kind) body.kind = form.kind;
  if (form.status !== saved.status) body.status = form.status;
  if (form.startsAt !== saved.startsAt) {
    body.starts_at = form.startsAt ? fromLocalInput(form.startsAt, timeZone) : null;
  }
  if (form.endsAt !== saved.endsAt) {
    body.ends_at = form.endsAt ? fromLocalInput(form.endsAt, timeZone) : null;
  }
  if (form.kind === "override" && form.compareAt !== saved.compareAt) {
    body.compare_at = form.compareAt;
  }
  const adjustment = adjustmentOf(form);
  if (!sameAdjustment(list.adjustment, adjustment)) body.adjustment = adjustment;
  // Replaced as a whole; `[]` is everyone again.
  const groups = form.groups.toSorted();
  if (JSON.stringify(groups) !== JSON.stringify(saved.groups.toSorted())) {
    body.audience = { groups };
  }
  return { body };
}

// ---- fixed prices' errors ----

/**
 * A message for one fixed price's problem, by the API's code, in `t`'s language; a code
 * the admin has no sentence for shows the API's own.
 */
export function entryMessage(t: PriceT, code: string, message: string): string {
  switch (code) {
    case "compare_at_not_above_price":
      return t("price_lists.entry_compare_at_not_above_price");
    case "too_many_price_breaks":
      return t("price_lists.breaks_too_many", { max: MAX_BREAKS });
    case "price_break_quantity_invalid":
      return t("price_lists.entry_price_break_quantity_invalid");
    case "price_breaks_not_descending":
      return t("price_lists.entry_price_breaks_not_descending");
    case "fixed_price_currency_mismatch":
    case "compare_at_currency_mismatch":
    case "price_break_currency_mismatch":
      return t("price_lists.entry_currency_mismatch");
    case "duplicate_fixed_price":
      return t("price_lists.entry_duplicate");
    case "variant_not_found":
    case "unknown_variant":
      return t("price_lists.entry_variant_not_found");
    default:
      return message ? `${message.charAt(0).toUpperCase()}${message.slice(1)}.` : code;
  }
}

/**
 * Why each entry of a refused `PUT …/fixed-prices` was refused, by variant, from a
 * `fixed_prices_invalid` answer's `details.entries[{index, variant, code, message}]`
 * (the index into `sent` when an entry names no variant). Every other entry sent with
 * them wasn't saved either: the request is all or nothing. `null` when the details
 * aren't that shape.
 */
export function entryErrors(
  t: PriceT,
  details: unknown,
  sent: readonly { variant: string }[],
): Record<string, string> | null {
  if (typeof details !== "object" || details === null) return null;
  const entries: unknown = Reflect.get(details, "entries");
  if (!Array.isArray(entries)) return null;
  const errors: Record<string, string> = {};
  for (const entry of entries) {
    if (typeof entry !== "object" || entry === null) continue;
    const index: unknown = Reflect.get(entry, "index");
    const named: unknown = Reflect.get(entry, "variant");
    const variant =
      typeof named === "string" ? named : typeof index === "number" ? sent[index]?.variant : null;
    if (!variant) continue;
    const code: unknown = Reflect.get(entry, "code");
    const message: unknown = Reflect.get(entry, "message");
    const text = entryMessage(
      t,
      typeof code === "string" ? code : "",
      typeof message === "string" ? message : "",
    );
    errors[variant] = errors[variant] ? `${errors[variant]} ${text}` : text;
  }
  return errors;
}

/** Catalog IDs from a `price_list_in_use` answer's `details.catalogs`. */
export function catalogsInUse(details: unknown): string[] {
  if (typeof details !== "object" || details === null) return [];
  const catalogs: unknown = Reflect.get(details, "catalogs");
  return Array.isArray(catalogs) ? catalogs.filter((c): c is string => typeof c === "string") : [];
}

// ---- the product page's prices ----

/**
 * Why a candidate list did or didn't set a variant's price, for the Prices card, in `t`'s
 * language. A result this build doesn't know as it is.
 */
export function candidateReason(result: string, phase: string, t: PriceT): string {
  switch (result) {
    case "won":
    case "higher":
    case "tied_with_older":
    case "not_pricing":
    case "not_in_audience":
      return t(`price_lists.candidate_${result}`);
    case "not_live":
      return t(
        phase === "scheduled"
          ? "price_lists.candidate_not_live_scheduled"
          : phase === "expired"
            ? "price_lists.candidate_not_live_expired"
            : "price_lists.candidate_not_live_draft",
      );
    default:
      return result;
  }
}
