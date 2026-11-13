// A discount draft in one line, "20% off Grow kits · Minimum €50 · Combines with
// shipping discounts · 1 use per customer": the instant preview the form shows while the
// server's own `summary()` (the words buyers, emails and agents get) is on its way. Parts
// that don't parse yet are left out rather than guessed. Its phrases are messages
// (`discounts.summary_*`), so the line is written in the staff member's language, with
// their formats for money, percentages, dates and lists.

import type { Format } from "../format.ts";
import {
  type BuyXGetYDraft,
  type DiscountDraft,
  type ItemSetDraft,
  type PerMarketText,
  type StandardDraft,
  type TieredDraft,
  type ValueDraft,
  amountMarkets,
  combinedClasses,
} from "./draft.ts";
import { type DiscountsT, type MarketOption, amountsOf, parseCount, parseOff } from "./validate.ts";

export type SummaryContext = {
  markets: readonly MarketOption[];
  currencies: Readonly<Record<string, number>>;
  /** Names by ID: products, variants, categories, channels, groups, customers. */
  names: Readonly<Record<string, string>>;
  /** Today in the store's zone (`2026-10-24`): dates in another year show theirs. */
  today?: string;
  t: DiscountsT;
  /** The staff member's formats. Money here is written from the draft's own currency
   *  table, so a `Format` without one will do. */
  format: Pick<Format, "number" | "percent" | "date" | "list">;
};

const SEPARATOR = " · ";

/** What stands for a number that doesn't parse yet. */
const UNKNOWN = "?";

/** "€50", "€12.50", "JP¥500": whole amounts without their zeros. */
export function shortMoney(
  amount: number,
  currency: string,
  digits: number,
  format: Pick<Format, "number">,
): string {
  const whole = amount % 10 ** digits === 0;
  return format.number(amount / 10 ** digits, {
    style: "currency",
    currency,
    minimumFractionDigits: whole ? 0 : digits,
    maximumFractionDigits: digits,
  });
}

/**
 * Amounts by market as one phrase: "€50" for one, "€50 or £45" for two currencies,
 * "€50 (and 2 more markets)" beyond. Markets sharing a currency and an amount count once.
 */
function amountsPhrase(
  amounts: PerMarketText,
  markets: readonly MarketOption[],
  ctx: SummaryContext,
): string | null {
  const parsed = amountsOf(amounts, markets, ctx.currencies);
  const shown: string[] = [];
  for (const market of markets) {
    const amount = parsed[market.id];
    const digits = ctx.currencies[market.currency];
    if (amount === undefined || digits === undefined) continue;
    const text = shortMoney(amount, market.currency, digits, ctx.format);
    if (!shown.includes(text)) shown.push(text);
  }
  const [first] = shown;
  if (first === undefined) return null;
  if (shown.length <= 2) return ctx.format.list(shown, "disjunction");
  return ctx.t("discounts.summary_amount_and_more", { amount: first, count: shown.length - 1 });
}

type Counted =
  | "products"
  | "variants"
  | "categories"
  | "markets"
  | "channels"
  | "groups"
  | "customers";

/** "Grow kits", "Grow kits and Cultures", "3 categories": a list by its names. */
function named(ids: readonly string[], ctx: SummaryContext, what: Counted): string {
  const names = ids.map((id) => ctx.names[id]).filter((name) => name !== undefined);
  if (ids.length > 0 && ids.length <= 2 && names.length === ids.length) {
    return ctx.format.list(names);
  }
  return ctx.t(`discounts.summary_count_${what}`, { count: ids.length });
}

/** What an item set covers: "every product", "Grow kits", "2 variants". */
function itemsPhrase(items: ItemSetDraft, ctx: SummaryContext): string {
  if (items.scope === "all") return ctx.t("discounts.summary_every_product");
  return named(items[items.scope], ctx, items.scope);
}

function percentText(value: ValueDraft, ctx: SummaryContext): string | null {
  const off = parseOff(value.percent);
  return off.ok ? ctx.format.percent(off.value / 10_000) : null;
}

function productReward(
  reward: NonNullable<StandardDraft["product"]>,
  markets: readonly MarketOption[],
  ctx: SummaryContext,
): string | null {
  const { t } = ctx;
  const { value, items } = reward;
  const target = itemsPhrase(items, ctx);
  const all = items.scope === "all";
  const text = ((): string | null => {
    switch (value.type) {
      case "percent": {
        const off = percentText(value, ctx);
        return off && t("discounts.summary_off_items", { off, items: target });
      }
      case "amount": {
        const off = amountsPhrase(value.amounts, markets, ctx);
        if (!off) return null;
        if (value.allocation === "each") {
          return all
            ? t("discounts.summary_off_each", { off })
            : t("discounts.summary_off_each_in", { off, items: target });
        }
        if (value.allocation === "cheapest") {
          const units = parseCount(value.cheapestUnits) ?? 1;
          return all
            ? t("discounts.summary_off_cheapest", { off, units })
            : t("discounts.summary_off_cheapest_in", { off, units, items: target });
        }
        return t("discounts.summary_off_items", { off, items: target });
      }
      case "fixed_price": {
        const price = amountsPhrase(value.amounts, markets, ctx);
        if (!price) return null;
        return all
          ? t("discounts.summary_fixed_price_all", { price })
          : t("discounts.summary_fixed_price", { price, items: target });
      }
    }
  })();
  return text && items.excludeOnSale ? t("discounts.summary_not_on_sale", { reward: text }) : text;
}

function offPhrase(value: ValueDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  return value.type === "percent"
    ? percentText(value, ctx)
    : amountsPhrase(value.amounts, markets, ctx);
}

function standard(kind: StandardDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  const { t } = ctx;
  const parts: string[] = [];
  if (kind.product) {
    const text = productReward(kind.product, markets, ctx);
    if (text) parts.push(text);
  }
  if (kind.order) {
    const off = offPhrase(kind.order.value, markets, ctx);
    if (off) parts.push(t("discounts.summary_off_order", { off }));
  }
  if (kind.shipping) {
    const value = kind.shipping.value;
    const cap = amountsPhrase(kind.shipping.maxAmount, markets, ctx);
    const percent = value.type === "percent" ? parseOff(value.percent) : null;
    if (percent?.ok && percent.value === 10_000) {
      parts.push(
        cap
          ? t("discounts.summary_free_shipping_capped", { cap })
          : t("discounts.summary_free_shipping"),
      );
    } else {
      const off = offPhrase(value, markets, ctx);
      if (off) {
        parts.push(
          cap
            ? t("discounts.summary_off_shipping_capped", { off, cap })
            : t("discounts.summary_off_shipping", { off }),
        );
      }
    }
  }
  return parts.join(" + ");
}

function buyXGetY(kind: BuyXGetYDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  const percent = kind.value.type === "percent" ? parseOff(kind.value.percent) : null;
  const uses = parseCount(kind.usesPerOrder);
  return ctx.t("discounts.summary_buy_x_get_y", {
    buy: kind.buy.type,
    buy_quantity: parseCount(kind.buy.quantity) ?? UNKNOWN,
    buy_amount: amountsPhrase(kind.buy.amounts, markets, ctx) ?? UNKNOWN,
    buys: itemsPhrase(kind.buys, ctx),
    get_quantity: parseCount(kind.getQuantity) ?? UNKNOWN,
    gets: itemsPhrase(kind.gets, ctx),
    reward:
      percent?.ok && percent.value === 10_000
        ? "free"
        : kind.value.type === "amount"
          ? "each"
          : "off",
    off: offPhrase(kind.value, markets, ctx) ?? UNKNOWN,
    limit: uses ? "some" : "none",
    uses: uses ?? 0,
  });
}

function tiered(kind: TieredDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  const tiers = kind.tiers.map((tier) =>
    ctx.t("discounts.summary_tier", {
      threshold:
        kind.measure === "quantity"
          ? (parseCount(tier.quantity) ?? UNKNOWN)
          : (amountsPhrase(tier.amounts, markets, ctx) ?? UNKNOWN),
      off: offPhrase(tier.value, markets, ctx) ?? UNKNOWN,
    }),
  );
  return ctx.t(`discounts.summary_tiered_${kind.measure}`, {
    tiers: ctx.format.list(tiers, "unit"),
    items: itemsPhrase(kind.items, ctx),
  });
}

/** "31 Oct", or "31 Oct 2027" in another year than `today`'s, from a wall-clock time. */
function dateText(local: string, ctx: SummaryContext): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(local);
  if (!match) return null;
  const [date = "", year = ""] = match;
  // The day as typed, whatever the zone: a calendar date, read at noon UTC.
  const noon = `${date}T12:00:00Z`;
  if (Number.isNaN(Date.parse(noon))) return null;
  return ctx.format.date(noon, ctx.today?.slice(0, 4) === year ? "short" : "long");
}

/** What the draft gives, its conditions, combinations, limits and window, as one line. */
export function draftSummary(draft: DiscountDraft, ctx: SummaryContext): string {
  const { t } = ctx;
  const markets = amountMarkets(draft.conditions, ctx.markets);
  const parts: string[] = [];

  const kind = draft.kind;
  const value =
    kind.type === "standard"
      ? standard(kind, markets, ctx)
      : kind.type === "buy_x_get_y"
        ? buyXGetY(kind, markets, ctx)
        : tiered(kind, markets, ctx);
  parts.push(value || t("discounts.summary_nothing_yet"));

  const c = draft.conditions;
  if (c.minSubtotal.enabled) {
    const amount = amountsPhrase(c.minSubtotal.amounts, markets, ctx);
    if (amount) parts.push(t("discounts.summary_min_subtotal", { amount }));
  }
  if (c.minQuantity.enabled) {
    const count = parseCount(c.minQuantity.quantity);
    if (count) parts.push(t("discounts.summary_min_quantity", { count }));
  }
  if (c.markets.length > 0) {
    parts.push(t("discounts.summary_in_markets", { markets: named(c.markets, ctx, "markets") }));
  }
  if (c.channels.length > 0) {
    parts.push(
      t("discounts.summary_on_channels", { channels: named(c.channels, ctx, "channels") }),
    );
  }
  if (c.customerGroups.length > 0) {
    parts.push(t("discounts.summary_for", { who: named(c.customerGroups, ctx, "groups") }));
  }
  if (c.customers.length > 0) {
    parts.push(t("discounts.summary_for", { who: named(c.customers, ctx, "customers") }));
  }
  if (c.signedIn) parts.push(t("discounts.summary_signed_in"));

  const combines = combinedClasses(draft);
  parts.push(
    combines.length === 0
      ? t("discounts.summary_combines_with_none")
      : t("discounts.summary_combines_with", {
          classes: ctx.format.list(combines.map((k) => t(`discounts.summary_class_${k}`))),
        }),
  );

  const total = parseCount(draft.limits.totalUses);
  if (total) parts.push(t("discounts.summary_total_uses", { count: total }));
  if (draft.limits.oncePerCustomer) parts.push(t("discounts.summary_once_per_customer"));

  const from = draft.schedule.startsAt ? dateText(draft.schedule.startsAt, ctx) : null;
  const until = draft.schedule.endsAt ? dateText(draft.schedule.endsAt, ctx) : null;
  if (from && until) parts.push(t("discounts.summary_window", { from, until }));
  else if (from) parts.push(t("discounts.summary_from", { date: from }));
  else if (until) parts.push(t("discounts.summary_until", { date: until }));

  if (draft.status === "draft") parts.push(t("common.draft"));
  return parts.join(SEPARATOR);
}
