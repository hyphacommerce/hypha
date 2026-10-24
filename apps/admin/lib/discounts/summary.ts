// A discount draft in one line, "20% off Grow kits · Minimum €50 · Combines with
// shipping discounts · 1 use per customer": the instant preview the form shows while the
// server's own `summary()` (the words buyers, emails and agents get) is on its way. Parts
// that don't parse yet are left out rather than guessed.

import {
  type BuyXGetYDraft,
  type DiscountClass,
  type DiscountDraft,
  type ItemSetDraft,
  type PerMarketText,
  type StandardDraft,
  type TieredDraft,
  type ValueDraft,
  amountMarkets,
  combinedClasses,
} from "./draft.ts";
import { type MarketOption, parseAmounts, parseCount, parseOff } from "./validate.ts";

export type SummaryContext = {
  markets: readonly MarketOption[];
  currencies: Readonly<Record<string, number>>;
  /** Names by ID: products, variants, categories, channels, groups, customers. */
  names: Readonly<Record<string, string>>;
  /** Today in the store's zone (`2026-10-24`): dates in another year show theirs. */
  today?: string;
};

const SEPARATOR = " · ";

/** "€50", "€12.50", "JP¥500": whole amounts without their zeros. */
export function shortMoney(amount: number, currency: string, digits: number): string {
  const whole = amount % 10 ** digits === 0;
  return new Intl.NumberFormat("en-IE", {
    style: "currency",
    currency,
    minimumFractionDigits: whole ? 0 : digits,
    maximumFractionDigits: digits,
  }).format(amount / 10 ** digits);
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
  const parsed = parseAmounts(amounts, markets, ctx.currencies, "").amounts;
  const shown: string[] = [];
  for (const market of markets) {
    const amount = parsed[market.id];
    const digits = ctx.currencies[market.currency];
    if (amount === undefined || digits === undefined) continue;
    const text = shortMoney(amount, market.currency, digits);
    if (!shown.includes(text)) shown.push(text);
  }
  const [first, second] = shown;
  if (first === undefined) return null;
  if (shown.length === 1) return first;
  if (shown.length === 2) return `${first} or ${second}`;
  return `${first} (and ${shown.length - 1} more markets)`;
}

/** "Grow kits", "Grow kits and Cultures", "3 categories": a list by its names. */
function named(ids: readonly string[], ctx: SummaryContext, one: string, many: string): string {
  if (ids.length === 0) return `no ${many}`;
  const names = ids.map((id) => ctx.names[id]);
  if (ids.length <= 2 && names.every((n) => n !== undefined)) return names.join(" and ");
  return ids.length === 1 ? `1 ${one}` : `${ids.length.toLocaleString("en")} ${many}`;
}

/** What an item set covers: "every product", "Grow kits", "2 variants". */
function itemsPhrase(items: ItemSetDraft, ctx: SummaryContext): string {
  switch (items.scope) {
    case "all":
      return "every product";
    case "products":
      return named(items.products, ctx, "product", "products");
    case "variants":
      return named(items.variants, ctx, "variant", "variants");
    case "categories":
      return named(items.categories, ctx, "category", "categories");
  }
}

function percentText(value: ValueDraft): string | null {
  const off = parseOff(value.percent);
  return off.ok ? `${off.value / 100}%` : null;
}

function productReward(
  reward: NonNullable<StandardDraft["product"]>,
  markets: readonly MarketOption[],
  ctx: SummaryContext,
): string | null {
  const { value, items } = reward;
  const target = itemsPhrase(items, ctx);
  const all = items.scope === "all";
  const unsold = items.excludeOnSale ? " not on sale" : "";
  switch (value.type) {
    case "percent": {
      const percent = percentText(value);
      return percent && `${percent} off ${target}${unsold}`;
    }
    case "amount": {
      const amount = amountsPhrase(value.amounts, markets, ctx);
      if (!amount) return null;
      const scope = all ? "" : ` in ${target}`;
      if (value.allocation === "each") return `${amount} off each item${scope}${unsold}`;
      if (value.allocation === "cheapest") {
        const units = parseCount(value.cheapestUnits) ?? 1;
        const cheapest = units === 1 ? "the cheapest item" : `the ${units} cheapest items`;
        return `${amount} off ${cheapest}${scope}${unsold}`;
      }
      return `${amount} off ${target}${unsold}`;
    }
    case "fixed_price": {
      const amount = amountsPhrase(value.amounts, markets, ctx);
      return amount && `${all ? "Every product" : target} for ${amount} each${unsold}`;
    }
  }
}

function offPhrase(value: ValueDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  return value.type === "percent" ? percentText(value) : amountsPhrase(value.amounts, markets, ctx);
}

function standard(kind: StandardDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  const parts: string[] = [];
  if (kind.product) {
    const text = productReward(kind.product, markets, ctx);
    if (text) parts.push(text);
  }
  if (kind.order) {
    const off = offPhrase(kind.order.value, markets, ctx);
    if (off) parts.push(`${off} off the order`);
  }
  if (kind.shipping) {
    const value = kind.shipping.value;
    const cap = amountsPhrase(kind.shipping.maxAmount, markets, ctx);
    const upTo = cap ? ` up to ${cap}` : "";
    const off = value.type === "percent" ? parseOff(value.percent) : null;
    if (off?.ok && off.value === 10_000) {
      parts.push(`Free shipping${upTo}`);
    } else {
      const phrase = offPhrase(value, markets, ctx);
      if (phrase) parts.push(`${phrase} off shipping${upTo}`);
    }
  }
  return parts.join(" + ");
}

function buyXGetY(kind: BuyXGetYDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  const buys = itemsPhrase(kind.buys, ctx);
  const gets = itemsPhrase(kind.gets, ctx);
  const buy =
    kind.buy.type === "quantity"
      ? `Buy ${parseCount(kind.buy.quantity) ?? "?"} of ${buys}`
      : `Spend ${amountsPhrase(kind.buy.amounts, markets, ctx) ?? "?"} on ${buys}`;
  const n = parseCount(kind.getQuantity) ?? "?";
  const percent = kind.value.type === "percent" ? parseOff(kind.value.percent) : null;
  const reward =
    percent?.ok && percent.value === 10_000
      ? "free"
      : `${offPhrase(kind.value, markets, ctx) ?? "?"} off${kind.value.type === "amount" ? " each" : ""}`;
  const uses = parseCount(kind.usesPerOrder);
  const limit = uses ? ` (up to ${uses} per order)` : "";
  return `${buy}, get ${n} of ${gets} ${reward}${limit}`;
}

function tiered(kind: TieredDraft, markets: readonly MarketOption[], ctx: SummaryContext) {
  const tiers = kind.tiers.map((tier) => {
    const threshold =
      kind.measure === "quantity"
        ? `${parseCount(tier.quantity) ?? "?"}+`
        : `${amountsPhrase(tier.amounts, markets, ctx) ?? "?"}+`;
    return `${threshold} ${offPhrase(tier.value, markets, ctx) ?? "?"} off`;
  });
  const lead = kind.measure === "quantity" ? "Buy" : "Spend";
  return `${lead} ${tiers.join(", ")} on ${itemsPhrase(kind.items, ctx)}`;
}

const CLASS_WORDS: Record<DiscountClass, string> = {
  product: "product",
  order: "order",
  shipping: "shipping",
};

/** "product and shipping", "product, order and shipping". */
function andList(words: readonly string[]): string {
  if (words.length <= 1) return words.join("");
  return `${words.slice(0, -1).join(", ")} and ${words.at(-1)}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "31 Oct", or "31 Oct 2027" in another year than `today`'s, from a wall-clock time. */
function dateText(local: string, today: string | undefined): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(local);
  if (!match) return null;
  const [, year = "", month = "", day = ""] = match;
  const name = MONTHS[Number(month) - 1];
  if (!name) return null;
  const sameYear = today?.slice(0, 4) === year;
  return `${Number(day)} ${name}${sameYear ? "" : ` ${year}`}`;
}

/** What the draft gives, its conditions, combinations, limits and window, as one line. */
export function draftSummary(draft: DiscountDraft, ctx: SummaryContext): string {
  const markets = amountMarkets(draft.conditions, ctx.markets);
  const parts: string[] = [];

  const kind = draft.kind;
  const value =
    kind.type === "standard"
      ? standard(kind, markets, ctx)
      : kind.type === "buy_x_get_y"
        ? buyXGetY(kind, markets, ctx)
        : tiered(kind, markets, ctx);
  parts.push(value || "Nothing off yet");

  const c = draft.conditions;
  if (c.minSubtotal.enabled) {
    const min = amountsPhrase(c.minSubtotal.amounts, markets, ctx);
    if (min) parts.push(`Minimum ${min}`);
  }
  if (c.minQuantity.enabled) {
    const n = parseCount(c.minQuantity.quantity);
    if (n) parts.push(`Minimum ${n} ${n === 1 ? "item" : "items"}`);
  }
  if (c.markets.length > 0) parts.push(`In ${named(c.markets, ctx, "market", "markets")}`);
  if (c.channels.length > 0) parts.push(`On ${named(c.channels, ctx, "channel", "channels")}`);
  if (c.customerGroups.length > 0) {
    parts.push(`For ${named(c.customerGroups, ctx, "customer group", "customer groups")}`);
  }
  if (c.customers.length > 0) parts.push(`For ${named(c.customers, ctx, "customer", "customers")}`);
  if (c.signedIn) parts.push("Signed-in customers only");

  const combines = combinedClasses(draft);
  parts.push(
    combines.length === 0
      ? "Doesn't combine with other discounts"
      : `Combines with ${andList(combines.map((k) => CLASS_WORDS[k]))} discounts`,
  );

  const total = parseCount(draft.limits.totalUses);
  if (total) parts.push(`${total.toLocaleString("en")} ${total === 1 ? "use" : "uses"} in total`);
  if (draft.limits.oncePerCustomer) parts.push("1 use per customer");

  const from = draft.schedule.startsAt ? dateText(draft.schedule.startsAt, ctx.today) : null;
  const until = draft.schedule.endsAt ? dateText(draft.schedule.endsAt, ctx.today) : null;
  if (from && until) parts.push(`${from} – ${until}`);
  else if (from) parts.push(`From ${from}`);
  else if (until) parts.push(`Until ${until}`);

  if (draft.status === "draft") parts.push("Draft");
  return parts.join(SEPARATOR);
}
