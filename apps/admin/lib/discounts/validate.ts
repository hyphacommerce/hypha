// What's wrong with a discount draft, every problem at once with its path in the draft
// ("kind.product.value.amounts.mkt_…"), as the API's `invalid_discount` lists its own;
// the API's problems mapped onto the same paths; and the paths turned into the field
// errors the form shows.

import { fromLocalInput } from "../price-lists.ts";
import { parseAmount, parsePercent } from "../price-grid/money.ts";
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
};

/** Field errors by draft path, the first problem of each. */
export type FieldErrors = Record<string, string>;

const count = (n: number) => n.toLocaleString("en");

/** A positive whole number from what was typed, or null. */
export function parseCount(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

/** Basis points off (2000 for "20"), from 0.01% to 100%, or what's wrong. */
export function parseOff(text: string): { ok: true; value: number } | { ok: false; error: string } {
  if (/^\s*[-−]/.test(text)) return { ok: false, error: "Without a sign: 20 is 20% off." };
  const parsed = parsePercent(text);
  if (!parsed.ok) return { ok: false, error: "A percentage, like 20 for 20% off." };
  if (parsed.value <= 0) return { ok: false, error: "More than 0%." };
  if (parsed.value > 10_000) return { ok: false, error: "At most 100%." };
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
  options: { allowZero?: boolean } = {},
): { amounts: Record<string, number>; problems: Problem[] } {
  const parsed: Record<string, number> = {};
  const problems: Problem[] = [];
  for (const market of markets) {
    const text = amounts[market.id]?.trim() ?? "";
    if (text === "") continue;
    const digits = currencies[market.currency];
    const at = `${path}.${market.id}`;
    if (digits === undefined) {
      problems.push({ path: at, message: `${market.currency} isn't a currency the store knows.` });
      continue;
    }
    const amount = parseAmount(text, digits, market.currency);
    if (!amount.ok) problems.push({ path: at, message: amount.error });
    else if (amount.value === 0 && !options.allowZero) {
      problems.push({ path: at, message: "More than 0." });
    } else parsed[market.id] = amount.value;
  }
  return { amounts: parsed, problems };
}

function checkAmounts(
  amounts: PerMarketText,
  ctx: CheckContext,
  markets: readonly MarketOption[],
  path: string,
  what: string,
  options: { allowZero?: boolean } = {},
): Problem[] {
  const parsed = parseAmounts(amounts, markets, ctx.currencies, path, options);
  const typed = markets.some((m) => (amounts[m.id]?.trim() ?? "") !== "");
  if (!typed) return [{ path, message: `Enter ${what} for at least one market.` }];
  return parsed.problems;
}

function checkValue(
  value: ValueDraft,
  ctx: CheckContext,
  markets: readonly MarketOption[],
  path: string,
  allowed: readonly ValueDraft["type"][],
): Problem[] {
  if (!allowed.includes(value.type)) {
    return [{ path: `${path}.type`, message: "This reward can't take that kind of value." }];
  }
  switch (value.type) {
    case "percent": {
      const off = parseOff(value.percent);
      return off.ok ? [] : [{ path: `${path}.percent`, message: off.error }];
    }
    case "amount": {
      const problems = checkAmounts(value.amounts, ctx, markets, `${path}.amounts`, "an amount");
      if (value.allocation === "cheapest" && parseCount(value.cheapestUnits) === null) {
        problems.push({ path: `${path}.cheapestUnits`, message: "A whole number, 1 or more." });
      }
      return problems;
    }
    case "fixed_price":
      return checkAmounts(value.amounts, ctx, markets, `${path}.amounts`, "a price", {
        allowZero: true,
      });
  }
}

function checkItems(items: ItemSetDraft, path: string): Problem[] {
  const named: Record<Exclude<ItemSetDraft["scope"], "all">, [string[], string]> = {
    products: [items.products, "products"],
    variants: [items.variants, "variants"],
    categories: [items.categories, "categories"],
  };
  if (items.scope === "all") return [];
  const [ids, noun] = named[items.scope];
  if (ids.length === 0) return [{ path: `${path}.${items.scope}`, message: `Choose the ${noun}.` }];
  if (ids.length > MAX_IDS) {
    return [{ path: `${path}.${items.scope}`, message: `At most ${count(MAX_IDS)} ${noun}.` }];
  }
  return [];
}

function checkIds(ids: readonly string[], path: string, noun: string): Problem[] {
  return ids.length > MAX_IDS ? [{ path, message: `At most ${count(MAX_IDS)} ${noun}.` }] : [];
}

function checkKind(draft: DiscountDraft, ctx: CheckContext): Problem[] {
  const markets = amountMarkets(draft.conditions, ctx.markets);
  const kind = draft.kind;
  const problems: Problem[] = [];
  switch (kind.type) {
    case "standard": {
      if (!kind.product && !kind.order && !kind.shipping) {
        problems.push({
          path: "kind",
          message: "Choose what it takes off: products, the order or shipping.",
        });
      }
      if (kind.product) {
        problems.push(...checkItems(kind.product.items, "kind.product.items"));
        problems.push(
          ...checkValue(kind.product.value, ctx, markets, "kind.product.value", [
            "percent",
            "amount",
            "fixed_price",
          ]),
        );
      }
      if (kind.order) {
        problems.push(
          ...checkValue(kind.order.value, ctx, markets, "kind.order.value", ["percent", "amount"]),
        );
      }
      if (kind.shipping) {
        problems.push(
          ...checkValue(kind.shipping.value, ctx, markets, "kind.shipping.value", [
            "percent",
            "amount",
          ]),
          ...parseAmounts(
            kind.shipping.maxAmount,
            markets,
            ctx.currencies,
            "kind.shipping.maxAmount",
          ).problems,
        );
      }
      return problems;
    }
    case "buy_x_get_y": {
      problems.push(...checkItems(kind.buys, "kind.buys"), ...checkItems(kind.gets, "kind.gets"));
      if (kind.buy.type === "quantity") {
        if (parseCount(kind.buy.quantity) === null) {
          problems.push({ path: "kind.buy.quantity", message: "A whole number, 1 or more." });
        }
      } else {
        problems.push(
          ...checkAmounts(kind.buy.amounts, ctx, markets, "kind.buy.amounts", "an amount"),
        );
      }
      if (parseCount(kind.getQuantity) === null) {
        problems.push({ path: "kind.getQuantity", message: "A whole number, 1 or more." });
      }
      problems.push(...checkValue(kind.value, ctx, markets, "kind.value", ["percent", "amount"]));
      if (kind.usesPerOrder.trim() !== "" && parseCount(kind.usesPerOrder) === null) {
        problems.push({ path: "kind.usesPerOrder", message: "A whole number, or blank for any." });
      }
      return problems;
    }
    case "tiered": {
      problems.push(...checkItems(kind.items, "kind.items"));
      if (kind.tiers.length === 0) problems.push({ path: "kind.tiers", message: "Add a tier." });
      if (kind.tiers.length > MAX_TIERS) {
        problems.push({ path: "kind.tiers", message: `At most ${MAX_TIERS} tiers.` });
      }
      let previous: number | null = null;
      const previousAmounts: Record<string, number> = {};
      kind.tiers.forEach((tier, i) => {
        const at = `kind.tiers.${i}`;
        if (kind.measure === "quantity") {
          const n = parseCount(tier.quantity);
          if (n === null) {
            problems.push({ path: `${at}.quantity`, message: "A whole number, 1 or more." });
          } else if (previous !== null && n <= previous) {
            problems.push({
              path: `${at}.quantity`,
              message: "Each tier needs more than the one before it.",
            });
          }
          if (n !== null) previous = n;
        } else {
          problems.push(...checkAmounts(tier.amounts, ctx, markets, `${at}.amounts`, "a subtotal"));
          const parsed = parseAmounts(tier.amounts, markets, ctx.currencies, `${at}.amounts`);
          for (const [market, amount] of Object.entries(parsed.amounts)) {
            const before = previousAmounts[market];
            if (before !== undefined && amount <= before) {
              problems.push({
                path: `${at}.amounts.${market}`,
                message: "Each tier needs more than the one before it.",
              });
            }
            previousAmounts[market] = amount;
          }
        }
        problems.push(
          ...checkValue(tier.value, ctx, markets, `${at}.value`, ["percent", "amount"]),
        );
      });
      return problems;
    }
  }
}

/** Every rule the draft breaks, in the form's order; empty when it can be sent. */
export function checkDraft(draft: DiscountDraft, ctx: CheckContext): Problem[] {
  const problems: Problem[] = [];
  const title = draft.title.trim();
  if (title === "") problems.push({ path: "title", message: "Give it a title buyers will see." });
  else if (title.length > MAX_TITLE) {
    problems.push({ path: "title", message: `At most ${MAX_TITLE} characters.` });
  }
  if (draft.method === "codes") {
    const code = draft.code.trim();
    if (/\s/.test(code)) problems.push({ path: "code", message: "A code has no spaces." });
    else if (code.length > MAX_CODE) {
      problems.push({ path: "code", message: `At most ${MAX_CODE} characters.` });
    }
  }

  problems.push(...checkKind(draft, ctx));

  const c = draft.conditions;
  problems.push(
    ...checkIds(c.markets, "conditions.markets", "markets"),
    ...checkIds(c.channels, "conditions.channels", "channels"),
    ...checkIds(c.customerGroups, "conditions.customerGroups", "customer groups"),
    ...checkIds(c.customers, "conditions.customers", "customers"),
  );
  if (c.minSubtotal.enabled) {
    problems.push(
      ...checkAmounts(
        c.minSubtotal.amounts,
        ctx,
        amountMarkets(c, ctx.markets),
        "conditions.minSubtotal.amounts",
        "a minimum",
      ),
    );
  }
  if (c.minQuantity.enabled && parseCount(c.minQuantity.quantity) === null) {
    problems.push({
      path: "conditions.minQuantity.quantity",
      message: "A whole number, 1 or more.",
    });
  }

  if (draft.limits.totalUses.trim() !== "" && parseCount(draft.limits.totalUses) === null) {
    problems.push({ path: "limits.totalUses", message: "A whole number, or blank for no limit." });
  }

  const { startsAt, endsAt } = draft.schedule;
  const starts = startsAt ? fromLocalInput(startsAt, ctx.timeZone) : null;
  const ends = endsAt ? fromLocalInput(endsAt, ctx.timeZone) : null;
  if (startsAt && !starts) {
    problems.push({ path: "schedule.startsAt", message: "Enter a date and a time." });
  }
  if (endsAt && !ends)
    problems.push({ path: "schedule.endsAt", message: "Enter a date and a time." });
  if (starts && ends && Date.parse(ends) <= Date.parse(starts)) {
    problems.push({ path: "schedule.endsAt", message: "The end must be after the start." });
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
