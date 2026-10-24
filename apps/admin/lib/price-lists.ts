// Price lists as the admin's screens use them: labels for kinds and phases, the
// adjustment's percentage and what it covers, the schedule in the store's time zone,
// the settings form and the requests it makes, and the fixed-price errors by row. Plain
// functions and types only, shared by server and client code (and run by `node --test`).

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { type CategoryNode, subtree } from "./categories.ts";
import type { Money } from "./price-grid/types.ts";

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

export const KINDS: { value: Kind; label: string; description: string }[] = [
  {
    value: "sale",
    label: "Sale",
    description:
      "A reduction: shoppers see its price struck through against the regular price, and the product counts as on sale.",
  },
  {
    value: "override",
    label: "Regular prices",
    description:
      "The regular price where it applies (a market's own prices, wholesale): nothing is struck through unless a compare-at price says so.",
  },
];

export function kindLabel(kind: string): string {
  return kind === "sale" ? "Sale" : kind === "override" ? "Regular prices" : kind;
}

export const PHASES: { value: Phase; label: string }[] = [
  { value: "active", label: "Active" },
  { value: "scheduled", label: "Scheduled" },
  { value: "expired", label: "Expired" },
  { value: "draft", label: "Draft" },
];

/** A phase's badge: its label and colour. */
export function phaseBadge(phase: string): {
  label: string;
  variant: "neutral" | "info" | "success" | "warning";
} {
  switch (phase) {
    case "active":
      return { label: "Active", variant: "success" };
    case "scheduled":
      return { label: "Scheduled", variant: "info" };
    case "expired":
      return { label: "Expired", variant: "warning" };
    case "draft":
      return { label: "Draft", variant: "neutral" };
    default:
      return { label: phase, variant: "neutral" };
  }
}

// ---- the adjustment ----

/**
 * Basis points from a typed percentage ("-20", "5.5", "+10"), or null when it isn't a
 * number with at most two decimals. Negative is a discount; -100 is the most off.
 */
export function parsePercent(input: string): number | null {
  const match = /^([-+−])?\s*(\d+)(?:[.,](\d{1,2}))?$/.exec(input.trim());
  if (!match) return null;
  const [, sign, whole, fraction = ""] = match;
  const bps = Number(whole) * 100 + Number(fraction.padEnd(2, "0"));
  const signed = sign === "-" || sign === "−" ? -bps : bps;
  if (!Number.isSafeInteger(signed) || signed < -10_000 || signed > 1_000_000) return null;
  return signed === 0 ? 0 : signed;
}

/** The percent input's text for `bps`: "-20" for -2000, "5.5" for 550. */
export function percentInput(bps: number): string {
  return String(bps / 100);
}

const PERCENT = new Intl.NumberFormat("en-IE", { maximumFractionDigits: 2, signDisplay: "always" });

/** "−20%" from -2000. */
export function percentLabel(bps: number): string {
  return `${PERCENT.format(bps / 100).replace("-", "−")}%`;
}

function count(n: number, one: string, many: string): string {
  return `${n.toLocaleString("en")} ${n === 1 ? one : many}`;
}

/** "−20% on every product", "−10% on 3 categories", "No adjustment". */
export function adjustmentLabel(adjustment: Adjustment | null | undefined): string {
  if (!adjustment) return "No adjustment";
  const percent = percentLabel(adjustment.percent_bps);
  const to = adjustment.applies_to ?? { type: "all" };
  switch (to.type) {
    case "products":
      return `${percent} on ${count(to.products.length, "product", "products")}`;
    case "categories":
      return `${percent} on ${count(to.categories.length, "category", "categories")}`;
    default:
      return `${percent} on every product`;
  }
}

/**
 * `amount` changed by `bps` basis points, rounded half away from zero in minor units:
 * the API's `Money::adjust`, so the grid shows what the store will charge. BigInt, as
 * an amount near 2⁵³ times 10,000 isn't exact in a double.
 */
export function adjustAmount(amount: number, bps: number): number {
  const scale = 10_000n;
  const numerator = BigInt(amount) * (scale + BigInt(bps));
  let result = numerator / scale;
  const remainder = numerator % scale;
  const twice = (remainder < 0n ? -remainder : remainder) * 2n;
  if (twice >= scale) result += numerator < 0n ? -1n : 1n;
  return Number(result);
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
  return { amount: adjustAmount(base.amount, adjustment.percent_bps), currency: base.currency };
}

// ---- the schedule, in the store's time zone ----

/** How far `timeZone`'s clock is ahead of UTC at instant `at`, in milliseconds. */
function zoneOffset(at: number, timeZone: string): number {
  const parts = new Intl.DateTimeFormat("en-US", {
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
  const parts = new Intl.DateTimeFormat("en-US", {
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
};

export type FormField =
  | "name"
  | "description"
  | "currency"
  | "startsAt"
  | "endsAt"
  | "percent"
  | "products"
  | "categories";

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
  };
}

export type TemplateKey = "sale" | "clearance" | "market" | "wholesale";

/** The new list page's starting points: each sets the fields its kind of list needs. */
export const TEMPLATES: {
  key: TemplateKey;
  label: string;
  description: string;
  apply: (form: ListForm, now: { startsAt: string; endsAt: string }) => ListForm;
}[] = [
  {
    key: "sale",
    label: "Sale",
    description: "A reduction for a while: 20% off a category, from one date to another.",
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
    label: "Clearance",
    description: "Sell off a category at a reduction, with no end date.",
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
    label: "Market prices",
    description:
      "A market's own regular prices: an adjustment of every base price, and fixed prices for some.",
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
    label: "Wholesale",
    description: "Fixed prices for the variants you sell in bulk, with quantity breaks.",
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

/** What's wrong with the form, by field; empty when it can be sent. */
export function checkForm(form: ListForm, timeZone: string): FormErrors {
  const errors: FormErrors = {};
  if (form.name.trim() === "") errors.name = "Give the list a name.";
  else if (form.name.trim().length > 255) errors.name = "At most 255 characters.";
  if (form.description.trim().length > 255) errors.description = "At most 255 characters.";
  if (form.currency === "") errors.currency = "Choose a currency.";
  const starts = form.startsAt ? fromLocalInput(form.startsAt, timeZone) : null;
  const ends = form.endsAt ? fromLocalInput(form.endsAt, timeZone) : null;
  if (form.startsAt && !starts) errors.startsAt = "Enter a date and a time.";
  if (form.endsAt && !ends) errors.endsAt = "Enter a date and a time.";
  if (starts && ends && Date.parse(ends) <= Date.parse(starts)) {
    errors.endsAt = "The end must be after the start.";
  }
  if (form.hasAdjustment) {
    if (parsePercent(form.percent) === null) {
      errors.percent = "A percentage, like -20 for 20% off or 5 for 5% more (at most 100% off).";
    }
    if (form.scope === "products" && form.products.length === 0) {
      errors.products = "Choose the products it applies to.";
    } else if (form.products.length > MAX_TARGETS) {
      errors.products = `At most ${MAX_TARGETS.toLocaleString("en")} products.`;
    }
    if (form.scope === "categories" && form.categories.length === 0) {
      errors.categories = "Choose the categories it applies to.";
    } else if (form.categories.length > MAX_TARGETS) {
      errors.categories = `At most ${MAX_TARGETS.toLocaleString("en")} categories.`;
    }
  }
  return errors;
}

function adjustmentOf(form: ListForm): Adjustment | null {
  if (!form.hasAdjustment) return null;
  const percent_bps = parsePercent(form.percent) ?? 0;
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
  form: ListForm,
  timeZone: string,
): { body: CreatePriceList } | { errors: FormErrors } {
  const errors = checkForm(form, timeZone);
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
  return { body };
}

/**
 * `PATCH /admin/price-lists/{id}`'s body: only what differs from `list` (JSON Merge
 * Patch: `null` clears), or what to fix first. An empty body means nothing changed.
 */
export function updateRequest(
  form: ListForm,
  list: PriceList,
  timeZone: string,
): { body: UpdatePriceList } | { errors: FormErrors } {
  const errors = checkForm(form, timeZone);
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
  return { body };
}

// ---- fixed prices' errors ----

/** A message for one fixed price's problem, by the API's code. */
export function entryMessage(code: string, message: string): string {
  switch (code) {
    case "compare_at_not_above_price":
      return "The compare-at price must be above the price.";
    case "too_many_price_breaks":
      return "At most 10 breaks.";
    case "price_break_quantity_invalid":
      return "Break quantities start at 2 and go up.";
    case "price_breaks_not_descending":
      return "Each break's price must be below the one before it, the first below the price.";
    case "fixed_price_currency_mismatch":
    case "compare_at_currency_mismatch":
    case "price_break_currency_mismatch":
      return "Prices must be in the list's currency.";
    case "duplicate_fixed_price":
      return "This variant is listed twice.";
    case "variant_not_found":
    case "unknown_variant":
      return "This variant no longer exists.";
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

/** Why a candidate list did or didn't set a variant's price, for the Prices card. */
export function candidateReason(result: string, phase: string): string {
  switch (result) {
    case "won":
      return "Wins: the lowest price.";
    case "higher":
      return "Loses: its price is higher.";
    case "tied_with_older":
      return "Loses: the same price as an older list, which wins the tie.";
    case "not_live":
      return phase === "scheduled"
        ? "Not live yet: its window hasn't started."
        : phase === "expired"
          ? "Not live: its window ended."
          : "Not live: it's a draft.";
    case "not_pricing":
      return "Doesn't price it: no fixed price, and its adjustment doesn't cover the product.";
    default:
      return result;
  }
}
