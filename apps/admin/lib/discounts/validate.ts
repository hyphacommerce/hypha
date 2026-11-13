// What's wrong with a discount draft, every problem at once with its path in the draft
// ("kind.product.value.amounts.mkt_…"), as the API's `invalid_discount` lists its own;
// the API's problems mapped onto the same paths; and the paths turned into the field
// errors the form shows.

import type { TFor } from "../messages";
import { fromLocalInput } from "../price-lists.ts";
import { amountOf, parseAmount, percentOf } from "../price-grid/money.ts";
import {
  type DiscountDraft,
  type ItemSetDraft,
  MAX_CODE,
  MAX_IDS,
  MAX_TIERS,
  MAX_TITLE,
  type PerMarketText,
  type ValueDraft,
  amountMarkets,
} from "./draft.ts";

/** The translator a discount's labels, problems and summary need. */
export type DiscountsT = TFor<"common" | "components" | "discounts">;

/** One broken rule: where it is and what to do about it. */
export type Problem = { path: string; message: string };

/** A market as amounts are typed for it. */
export type MarketOption = { id: string; name: string; currency: string };

export type CheckContext = {
  markets: readonly MarketOption[];
  /** Minor-unit digits by currency code (`useCurrencies`). */
  currencies: Readonly<Record<string, number>>;
  /** The store's time zone, which the schedule's wall-clock times are in. */
  timeZone: string;
  /** Says the problems in the staff member's language. */
  t: DiscountsT;
};

/** Field errors by draft path, the first problem of each. */
export type FieldErrors = Record<string, string>;

/** A positive whole number from what was typed, or null. */
export function parseCount(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

/** What's wrong with a percentage off, as the message saying so. */
export type OffProblem =
  | "discounts.problem_off_sign"
  | "discounts.problem_off_not_percent"
  | "discounts.problem_off_zero"
  | "discounts.problem_off_over";

/**
 * Basis points off (2000 for "20"), from 0.01% to 100%, or what's wrong: a message key,
 * for `t`.
 */
export function parseOff(
  text: string,
): { ok: true; value: number } | { ok: false; error: OffProblem } {
  if (/^\s*[-−]/.test(text)) return { ok: false, error: "discounts.problem_off_sign" };
  const value = percentOf(text);
  if (value === null) return { ok: false, error: "discounts.problem_off_not_percent" };
  if (value <= 0) return { ok: false, error: "discounts.problem_off_zero" };
  if (value > 10_000) return { ok: false, error: "discounts.problem_off_over" };
  return { ok: true, value };
}

/**
 * Minor units by market from typed amounts, for the markets amounts are typed for: those
 * that parse (zero only when allowed), the rest left out.
 */
export function amountsOf(
  amounts: PerMarketText,
  markets: readonly MarketOption[],
  currencies: CheckContext["currencies"],
  options: { allowZero?: boolean } = {},
): Record<string, number> {
  const parsed: Record<string, number> = {};
  for (const market of markets) {
    const text = amounts[market.id]?.trim() ?? "";
    const digits = currencies[market.currency];
    if (text === "" || digits === undefined) continue;
    const amount = amountOf(text, digits, market.currency);
    if (amount !== null && (amount !== 0 || options.allowZero)) parsed[market.id] = amount;
  }
  return parsed;
}

/**
 * Minor units by market from typed amounts, for the markets amounts are typed for; the
 * problems under `path` for those that don't parse.
 */
export function parseAmounts(
  amounts: PerMarketText,
  markets: readonly MarketOption[],
  currencies: CheckContext["currencies"],
  path: string,
  t: DiscountsT,
  options: { allowZero?: boolean } = {},
): { amounts: Record<string, number>; problems: Problem[] } {
  const problems: Problem[] = [];
  for (const market of markets) {
    const text = amounts[market.id]?.trim() ?? "";
    if (text === "") continue;
    const digits = currencies[market.currency];
    const at = `${path}.${market.id}`;
    if (digits === undefined) {
      problems.push({
        path: at,
        message: t("discounts.problem_unknown_currency", { currency: market.currency }),
      });
      continue;
    }
    const amount = parseAmount(text, digits, market.currency, t);
    if (!amount.ok) problems.push({ path: at, message: amount.error });
    else if (amount.value === 0 && !options.allowZero) {
      problems.push({ path: at, message: t("discounts.problem_amount_zero") });
    }
  }
  return { amounts: amountsOf(amounts, markets, currencies, options), problems };
}

function checkAmounts(
  amounts: PerMarketText,
  ctx: CheckContext,
  markets: readonly MarketOption[],
  path: string,
  what: "amount" | "price" | "subtotal" | "minimum",
  t: DiscountsT,
  options: { allowZero?: boolean } = {},
): Problem[] {
  const parsed = parseAmounts(amounts, markets, ctx.currencies, path, t, options);
  const typed = markets.some((m) => (amounts[m.id]?.trim() ?? "") !== "");
  if (!typed) return [{ path, message: t("discounts.problem_amounts_missing", { what }) }];
  return parsed.problems;
}

function checkValue(
  value: ValueDraft,
  ctx: CheckContext,
  markets: readonly MarketOption[],
  path: string,
  allowed: readonly ValueDraft["type"][],
  t: DiscountsT,
): Problem[] {
  if (!allowed.includes(value.type)) {
    return [{ path: `${path}.type`, message: t("discounts.problem_value_type") }];
  }
  switch (value.type) {
    case "percent": {
      const off = parseOff(value.percent);
      return off.ok ? [] : [{ path: `${path}.percent`, message: t(off.error) }];
    }
    case "amount": {
      const problems = checkAmounts(value.amounts, ctx, markets, `${path}.amounts`, "amount", t);
      if (value.allocation === "cheapest" && parseCount(value.cheapestUnits) === null) {
        problems.push({
          path: `${path}.cheapestUnits`,
          message: t("discounts.problem_whole_number"),
        });
      }
      return problems;
    }
    case "fixed_price":
      return checkAmounts(value.amounts, ctx, markets, `${path}.amounts`, "price", t, {
        allowZero: true,
      });
  }
}

/** What a list of IDs names, as `discounts.problem_too_many` tells them apart. */
type Listed =
  | "products"
  | "variants"
  | "categories"
  | "markets"
  | "channels"
  | "customer_groups"
  | "customers";

function checkItems(items: ItemSetDraft, path: string, t: DiscountsT): Problem[] {
  if (items.scope === "all") return [];
  const ids = items[items.scope];
  const at = `${path}.${items.scope}`;
  if (ids.length === 0) {
    return [{ path: at, message: t("discounts.problem_choose", { what: items.scope }) }];
  }
  return checkIds(ids, at, items.scope, t);
}

function checkIds(ids: readonly string[], path: string, what: Listed, t: DiscountsT): Problem[] {
  return ids.length > MAX_IDS
    ? [{ path, message: t("discounts.problem_too_many", { what, max: MAX_IDS }) }]
    : [];
}

function checkKind(draft: DiscountDraft, ctx: CheckContext): Problem[] {
  const t = ctx.t;
  const markets = amountMarkets(draft.conditions, ctx.markets);
  const kind = draft.kind;
  const problems: Problem[] = [];
  const wholeNumber = t("discounts.problem_whole_number");
  const tierOrder = t("discounts.problem_tier_order");
  switch (kind.type) {
    case "standard": {
      if (!kind.product && !kind.order && !kind.shipping) {
        problems.push({
          path: "kind",
          message: t("discounts.problem_kind_empty"),
        });
      }
      if (kind.product) {
        problems.push(...checkItems(kind.product.items, "kind.product.items", t));
        problems.push(
          ...checkValue(
            kind.product.value,
            ctx,
            markets,
            "kind.product.value",
            ["percent", "amount", "fixed_price"],
            t,
          ),
        );
      }
      if (kind.order) {
        problems.push(
          ...checkValue(
            kind.order.value,
            ctx,
            markets,
            "kind.order.value",
            ["percent", "amount"],
            t,
          ),
        );
      }
      if (kind.shipping) {
        problems.push(
          ...checkValue(
            kind.shipping.value,
            ctx,
            markets,
            "kind.shipping.value",
            ["percent", "amount"],
            t,
          ),
          ...parseAmounts(
            kind.shipping.maxAmount,
            markets,
            ctx.currencies,
            "kind.shipping.maxAmount",
            t,
          ).problems,
        );
      }
      return problems;
    }
    case "buy_x_get_y": {
      problems.push(
        ...checkItems(kind.buys, "kind.buys", t),
        ...checkItems(kind.gets, "kind.gets", t),
      );
      if (kind.buy.type === "quantity") {
        if (parseCount(kind.buy.quantity) === null) {
          problems.push({ path: "kind.buy.quantity", message: wholeNumber });
        }
      } else {
        problems.push(
          ...checkAmounts(kind.buy.amounts, ctx, markets, "kind.buy.amounts", "amount", t),
        );
      }
      if (parseCount(kind.getQuantity) === null) {
        problems.push({ path: "kind.getQuantity", message: wholeNumber });
      }
      problems.push(
        ...checkValue(kind.value, ctx, markets, "kind.value", ["percent", "amount"], t),
      );
      if (kind.usesPerOrder.trim() !== "" && parseCount(kind.usesPerOrder) === null) {
        problems.push({
          path: "kind.usesPerOrder",
          message: t("discounts.problem_uses_per_order"),
        });
      }
      return problems;
    }
    case "tiered": {
      problems.push(...checkItems(kind.items, "kind.items", t));
      if (kind.tiers.length === 0) {
        problems.push({ path: "kind.tiers", message: t("discounts.problem_tiers_empty") });
      }
      if (kind.tiers.length > MAX_TIERS) {
        problems.push({
          path: "kind.tiers",
          message: t("discounts.problem_tiers_too_many", { max: MAX_TIERS }),
        });
      }
      let previous: number | null = null;
      const previousAmounts: Record<string, number> = {};
      kind.tiers.forEach((tier, i) => {
        const at = `kind.tiers.${i}`;
        if (kind.measure === "quantity") {
          const n = parseCount(tier.quantity);
          if (n === null) {
            problems.push({ path: `${at}.quantity`, message: wholeNumber });
          } else if (previous !== null && n <= previous) {
            problems.push({ path: `${at}.quantity`, message: tierOrder });
          }
          if (n !== null) previous = n;
        } else {
          problems.push(
            ...checkAmounts(tier.amounts, ctx, markets, `${at}.amounts`, "subtotal", t),
          );
          const parsed = parseAmounts(tier.amounts, markets, ctx.currencies, `${at}.amounts`, t);
          for (const [market, amount] of Object.entries(parsed.amounts)) {
            const before = previousAmounts[market];
            if (before !== undefined && amount <= before) {
              problems.push({ path: `${at}.amounts.${market}`, message: tierOrder });
            }
            previousAmounts[market] = amount;
          }
        }
        problems.push(
          ...checkValue(tier.value, ctx, markets, `${at}.value`, ["percent", "amount"], t),
        );
      });
      return problems;
    }
  }
}

/** Every rule the draft breaks, in the form's order; empty when it can be sent. */
export function checkDraft(draft: DiscountDraft, ctx: CheckContext): Problem[] {
  const t = ctx.t;
  const problems: Problem[] = [];
  const title = draft.title.trim();
  if (title === "") problems.push({ path: "title", message: t("discounts.problem_title_empty") });
  else if (title.length > MAX_TITLE) {
    problems.push({
      path: "title",
      message: t("discounts.problem_too_long", { max: MAX_TITLE }),
    });
  }
  if (draft.method === "codes") {
    const code = draft.code.trim();
    if (/\s/.test(code)) {
      problems.push({ path: "code", message: t("discounts.problem_code_spaces") });
    } else if (code.length > MAX_CODE) {
      problems.push({ path: "code", message: t("discounts.problem_too_long", { max: MAX_CODE }) });
    }
  }

  problems.push(...checkKind(draft, ctx));

  const c = draft.conditions;
  problems.push(
    ...checkIds(c.markets, "conditions.markets", "markets", t),
    ...checkIds(c.channels, "conditions.channels", "channels", t),
    ...checkIds(c.customerGroups, "conditions.customerGroups", "customer_groups", t),
    ...checkIds(c.customers, "conditions.customers", "customers", t),
  );
  if (c.minSubtotal.enabled) {
    problems.push(
      ...checkAmounts(
        c.minSubtotal.amounts,
        ctx,
        amountMarkets(c, ctx.markets),
        "conditions.minSubtotal.amounts",
        "minimum",
        t,
      ),
    );
  }
  if (c.minQuantity.enabled && parseCount(c.minQuantity.quantity) === null) {
    problems.push({
      path: "conditions.minQuantity.quantity",
      message: t("discounts.problem_whole_number"),
    });
  }

  if (draft.limits.totalUses.trim() !== "" && parseCount(draft.limits.totalUses) === null) {
    problems.push({ path: "limits.totalUses", message: t("discounts.problem_total_uses") });
  }

  const { startsAt, endsAt } = draft.schedule;
  const starts = startsAt ? fromLocalInput(startsAt, ctx.timeZone) : null;
  const ends = endsAt ? fromLocalInput(endsAt, ctx.timeZone) : null;
  if (startsAt && !starts) {
    problems.push({ path: "schedule.startsAt", message: t("discounts.problem_date_time") });
  }
  if (endsAt && !ends) {
    problems.push({ path: "schedule.endsAt", message: t("discounts.problem_date_time") });
  }
  if (starts && ends && Date.parse(ends) <= Date.parse(starts)) {
    problems.push({ path: "schedule.endsAt", message: t("discounts.problem_end_before_start") });
  }
  return problems;
}

// ---- the API's problems ----

/**
 * Where a path in the API's body lands in the draft, or null when the form has no field
 * for it (its message is then shown above the form). The page passes one that knows the
 * wire shape; `problemsFromDetails` falls back to `defaultApiPath`.
 */
export type PathMapper = (apiPath: string) => string | null;

const API_FIELDS: Record<string, string> = {
  title: "title",
  code: "code",
  codes: "code",
  status: "status",
  method: "method",
  starts_at: "schedule.startsAt",
  ends_at: "schedule.endsAt",
  combines_with: "combinesWith",
  "limits.total": "limits.totalUses",
  "limits.total_uses": "limits.totalUses",
  "limits.once_per_customer": "limits.oncePerCustomer",
};

/**
 * The draft path for an API path in its plain fields ("title", "ends_at",
 * "limits.total_uses", "codes[0]"), else null. Paths may be dotted or JSON-pointer-like
 * ("/limits/total_uses") and may start with "$.".
 */
export function defaultApiPath(apiPath: string): string | null {
  const dotted = apiPath
    .replace(/^\$\.?/, "")
    .replace(/^\//, "")
    .replaceAll("/", ".")
    .replaceAll(/\[(\d+)\]/g, ".$1");
  const exact = API_FIELDS[dotted];
  if (exact) return exact;
  const head = dotted.split(".")[0] ?? "";
  return head === "codes" ? "code" : null;
}

/**
 * The problems in an `invalid_discount` answer's `details`: an array of
 * `{path, message}`, or an object holding one under `problems` or `errors`, or a single
 * `{path, message}`. Paths are mapped onto the draft's by `map`; one it can't place is
 * kept with the path "" (the whole form). Anything else in `details` is ignored.
 */
export function problemsFromDetails(details: unknown, map: PathMapper = defaultApiPath): Problem[] {
  const entries = listOf(details);
  const problems: Problem[] = [];
  for (const entry of entries) {
    if (typeof entry !== "object" || entry === null) continue;
    const path: unknown = Reflect.get(entry, "path");
    const message: unknown = Reflect.get(entry, "message");
    if (typeof message !== "string" || message === "") continue;
    const at = typeof path === "string" ? (map(path) ?? defaultApiPath(path)) : null;
    problems.push({ path: at ?? "", message: sentence(message) });
  }
  return problems;
}

function listOf(details: unknown): unknown[] {
  if (Array.isArray(details)) return details;
  if (typeof details !== "object" || details === null) return [];
  for (const key of ["problems", "errors"]) {
    const inner: unknown = Reflect.get(details, key);
    if (Array.isArray(inner)) return inner;
  }
  return "message" in details ? [details] : [];
}

/** The API's messages are lowercase clauses; the form shows sentences. */
function sentence(message: string): string {
  const capital = `${message.charAt(0).toUpperCase()}${message.slice(1)}`;
  return /[.!?]$/.test(capital) ? capital : `${capital}.`;
}

/** The first problem at each path, for the fields; whole-form problems under "". */
export function fieldErrors(problems: readonly Problem[]): FieldErrors {
  const errors: FieldErrors = {};
  for (const p of problems) errors[p.path] ??= p.message;
  return errors;
}

/** The error at `path` or, failing that, the first one below it (a section's summary). */
export function errorAt(errors: FieldErrors, path: string): string | undefined {
  if (errors[path]) return errors[path];
  const prefix = `${path}.`;
  for (const [key, message] of Object.entries(errors)) if (key.startsWith(prefix)) return message;
  return undefined;
}

/** `errors` without those at or below `paths`: the fields being changed are being fixed. */
export function clearErrors(errors: FieldErrors, paths: readonly string[]): FieldErrors {
  const next: FieldErrors = {};
  for (const [key, message] of Object.entries(errors)) {
    const cleared = paths.some((p) => key === p || key.startsWith(`${p}.`));
    if (!cleared) next[key] = message;
  }
  return next;
}
