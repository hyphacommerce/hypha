// Discounts as shoppers read them: a cart's codes and what each one's state means, the
// discounts applied with what they took off each line and the shipping, the totals'
// "Discounts" row, "Free" shipping and the promotion badges on products. Pure, so
// servers and browsers share it.
//
// The `read*` functions are the one place that knows the API's field names. The SDK
// doesn't type these fields yet, so they read the answer defensively and give "no
// discounts" for anything missing: pages render as before until the API sends them.

import { type Money, formatMoney } from "./money.ts";
import { lineCompareAt } from "./prices.ts";

/** Why a code the cart keeps does or doesn't apply now. */
export type CodeState =
  | { state: "applied" }
  /** A condition fails: `markets`, `channels`, `customer_groups`, `customers`,
   * `signed_in`. Open: an unknown one gets a general message. */
  | { state: "not_eligible"; condition: string }
  /** Short of a minimum, with what's missing. */
  | { state: "minimum_not_met"; needed: { subtotal?: Money; quantity?: number } }
  | { state: "no_matching_items" }
  /** A better combination excludes it: the discount (ID) it lost to. */
  | { state: "blocked_by_other"; discount: string }
  | { state: "limit_reached" }
  | { state: "not_available_in_market" }
  /** A state this storefront doesn't know yet. */
  | { state: "unknown" };

/** A code on the cart (normalised, as the API keeps it) and its state. */
export type CartCode = { code: string } & CodeState;

/** One discount applied: its title and summary for the buyer, and what it took off. */
export type Application = {
  index: number;
  discount: string;
  code: string | null;
  title: string;
  summary: string;
  amount: Money;
};

/** What one application took off a line or a shipment. */
export type Allocation = { application: number; amount: Money };

/** A line's discounts: what it costs before and after them. */
export type LineDiscount = { before: Money; after: Money; allocations: Allocation[] };

/** A shipment's price before and after a shipping discount. */
export type Shipment = { before: Money; after: Money };

/** Everything a cart or an order says about discounts. */
export type Discounts = {
  /** Whether the cart takes codes: it answers with `discount_codes`. Until the API
   * does, there's no code field to offer. */
  takesCodes: boolean;
  codes: CartCode[];
  applications: Application[];
  /** The sum of the applications: lines and shipping. */
  total: Money | null;
  /** What came off shipping, of `total`. */
  shipping: Money | null;
  /** By line key: the variant on a cart, the line's ID on an order. */
  lines: Record<string, LineDiscount>;
  /** By shipping group. */
  shipments: Record<string, Shipment>;
};

/** A product's live automatic discount, shown as a badge, never as a price. */
export type Promotion = { title: string; summary: string };

export const NO_DISCOUNTS: Discounts = {
  takesCodes: false,
  codes: [],
  applications: [],
  total: null,
  shipping: null,
  lines: {},
  shipments: {},
};

// ---- Reading the API's answers --------------------------------------------------------

type Json = Record<string, unknown>;

function object(value: unknown): Json | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- a plain object; every field read is checked
  return value as Json;
}

function list(value: unknown): Json[] {
  if (!Array.isArray(value)) return [];
  const found: Json[] = [];
  for (const v of value as unknown[]) {
    const o = object(v);
    if (o) found.push(o);
  }
  return found;
}

/** What came off shipping across the shipments, `null` when nothing did. */
function shippingOff(shipments: Record<string, Shipment>): Money | null {
  const all = Object.values(shipments);
  if (all.length === 0) return null;
  const amount = all.reduce((n, s) => n + s.before.amount - s.after.amount, 0);
  return { amount, currency: all[0].before.currency };
}

function money(value: unknown): Money | null {
  const v = object(value);
  return v && typeof v.amount === "number" && typeof v.currency === "string"
    ? { amount: v.amount, currency: v.currency }
    : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function allocations(value: unknown): Allocation[] {
  return list(value).flatMap((a) => {
    const amount = money(a.amount);
    return typeof a.application === "number" && amount
      ? [{ application: a.application, amount }]
      : [];
  });
}

/** A code's state, written either flat (`{code, state: "minimum_not_met", needed}`) or
 * as a tagged object (`{code, state: {type: "minimum_not_met", needed}}`). */
export function readCodeState(entry: Json): CodeState {
  const nested = object(entry.state);
  const fields = nested ?? entry;
  const name = nested ? text(nested.type) : text(entry.state);
  switch (name) {
    case "applied":
    case "no_matching_items":
    case "limit_reached":
    case "not_available_in_market":
      return { state: name };
    case "not_eligible":
      return { state: name, condition: text(fields.condition) ?? "" };
    case "minimum_not_met": {
      const needed = object(fields.needed) ?? {};
      return {
        state: name,
        needed: {
          subtotal: money(needed.subtotal) ?? undefined,
          quantity: typeof needed.quantity === "number" ? needed.quantity : undefined,
        },
      };
    }
    case "blocked_by_other":
      return { state: name, discount: text(fields.discount) ?? "" };
    default:
      return { state: "unknown" };
  }
}

function applications(value: unknown): Application[] {
  return list(value).flatMap((a, i) => {
    const amount = money(a.amount);
    const title = text(a.title);
    if (!amount || title === null) return [];
    return [
      {
        index: typeof a.index === "number" ? a.index : i,
        discount: text(a.discount) ?? "",
        code: text(a.code),
        title,
        summary: text(a.summary) ?? "",
        amount,
      },
    ];
  });
}

/** What a cart (`GET /storefront/carts/{id}`) says about discounts: `discount_codes`,
 * `discounts`, `discount_total`, each line's `allocations` and `total_after_discounts`
 * (its `total` being before them), each shipping group's `allocations`. */
export function readCartDiscounts(cart: unknown): Discounts {
  const c = object(cart);
  if (!c) return NO_DISCOUNTS;
  const codes = list(c.discount_codes).flatMap((entry) => {
    const code = text(entry.code);
    return code === null ? [] : [{ code, ...readCodeState(entry) }];
  });
  const lines: Record<string, LineDiscount> = {};
  for (const line of list(c.lines)) {
    const key = text(line.variant);
    const before = money(line.total);
    const after = money(line.total_after_discounts);
    if (key && before && after)
      lines[key] = { before, after, allocations: allocations(line.allocations) };
  }
  const shipments: Record<string, Shipment> = {};
  for (const group of list(c.shipping_groups)) {
    const key = text(group.id);
    const taken = allocations(group.allocations);
    const before = money(object(group.selected)?.amount);
    if (!key || !before || taken.length === 0) continue;
    const off = sum(taken.map((a) => a.amount));
    shipments[key] = { before, after: { ...before, amount: before.amount - off } };
  }
  return {
    takesCodes: Array.isArray(c.discount_codes),
    codes,
    applications: applications(c.discounts),
    total: money(c.discount_total),
    shipping: shippingOff(shipments),
    lines,
    shipments,
  };
}

/** What a placed order says: `discounts`, `discount_total`, each line's `subtotal`
 * (before), `total` (after) and `discounts`, each shipping line's `amount`, `total` and
 * `discounts`. An order from before discounts has none of them. */
export function readOrderDiscounts(order: unknown): Discounts {
  const o = object(order);
  if (!o) return NO_DISCOUNTS;
  const lines: Record<string, LineDiscount> = {};
  for (const line of list(o.lines)) {
    const key = text(line.id);
    const before = money(line.subtotal);
    const after = money(line.total);
    const taken = allocations(line.discounts);
    if (key && before && after && taken.length > 0)
      lines[key] = { before, after, allocations: taken };
  }
  const shipments: Record<string, Shipment> = {};
  for (const s of list(o.shipping_lines)) {
    const key = text(s.group);
    const before = money(s.amount);
    const after = money(s.total);
    if (!key || !before || !after || allocations(s.discounts).length === 0) continue;
    shipments[key] = { before, after };
  }
  return {
    takesCodes: false,
    codes: [],
    applications: applications(o.discounts),
    total: money(o.discount_total),
    shipping: shippingOff(shipments),
    lines,
    shipments,
  };
}

/** A product's (or a search hit's) `promotions`. */
export function readPromotions(product: unknown): Promotion[] {
  const p = object(product);
  return list(p?.promotions).flatMap((promo) => {
    const title = text(promo.title);
    return title ? [{ title, summary: text(promo.summary) ?? "" }] : [];
  });
}

// ---- What's shown ---------------------------------------------------------------------

function sum(amounts: Money[]): number {
  return amounts.reduce((n, m) => n + m.amount, 0);
}

/** "−€2.40": an amount taken off (a minus sign, not a hyphen). */
export function minus(amount: Money, digits: number): string {
  return `−${formatMoney(amount, digits)}`;
}

/** What a line shows: its total after discounts, the total before struck through, and a
 * note per discount ("−€2.40 Welcome offer"). Without discounts, `price` is `total`. */
export function lineView(
  total: Money,
  line: LineDiscount | undefined,
  applied: Application[],
  digits: number,
): { price: Money; struck: Money | null; notes: string[] } {
  if (!line || line.after.amount >= line.before.amount) {
    return { price: line?.after ?? total, struck: null, notes: [] };
  }
  const notes = line.allocations
    .filter((a) => a.amount.amount > 0)
    .map((a) => {
      const title = applied.find((x) => x.index === a.application)?.title ?? "Discount";
      return `${minus(a.amount, digits)} ${title}`;
    });
  return { price: line.after, struck: line.before, notes };
}

/** A cart or order line's price as shown: after discounts, struck through beside the
 * total before them, else beside its compare-at total (a sale) as before. */
export function linePrice(
  line: { total: Money; quantity: number; compare_at_price?: Money | null },
  discount: LineDiscount | undefined,
  applied: Application[],
  digits: number,
): { price: Money; compareAt: Money | null; notes: string[] } {
  const view = lineView(line.total, discount, applied, digits);
  return {
    price: view.price,
    compareAt: view.struck ?? lineCompareAt(line.compare_at_price, line.quantity, view.price),
    notes: view.notes,
  };
}

/** The totals with a "Discounts" row: the subtotal shown before what came off the goods,
 * so the rows still add up to the total; shipping's discount shows on shipping itself
 * ("Free" or struck through), not in the row. `subtotal` is after discounts, as the API
 * sends it. */
export function discountedTotals(
  subtotal: Money,
  discounts: Pick<Discounts, "total" | "shipping">,
): { subtotal: Money; goods: Money | null } {
  const off = (discounts.total?.amount ?? 0) - (discounts.shipping?.amount ?? 0);
  if (off <= 0) return { subtotal, goods: null };
  return {
    subtotal: { ...subtotal, amount: subtotal.amount + off },
    goods: { amount: off, currency: subtotal.currency },
  };
}

/** A shipping price: "Free" when a discount took all of it, else the amount, with the
 * price before a discount struck through. A rate that's free anyway stays "€0.00". */
export function shippingPrice(
  amount: Money,
  before: Money | null | undefined,
  digits: number,
): { text: string; was: string | null } {
  const discounted = before && before.amount > amount.amount ? before : null;
  return {
    text: discounted && amount.amount <= 0 ? "Free" : formatMoney(amount, digits),
    was: discounted ? formatMoney(discounted, digits) : null,
  };
}

/** What a code's state tells the shopper, and whether it's good news. */
export function codeMessage(
  code: CartCode,
  applied: Application[],
  digits: number,
): { text: string; tone: "success" | "warning" } {
  const c = code.code;
  switch (code.state) {
    case "applied": {
      const title = applied.find((a) => a.code === c)?.title;
      return { text: title ? `Applied: ${title}` : "Applied", tone: "success" };
    }
    case "minimum_not_met": {
      const { subtotal, quantity } = code.needed;
      if (subtotal && subtotal.amount > 0) {
        return { text: `Add ${formatMoney(subtotal, digits)} more to use ${c}`, tone: "warning" };
      }
      if (quantity && quantity > 0) {
        const items = quantity === 1 ? "1 more item" : `${quantity} more items`;
        return { text: `Add ${items} to use ${c}`, tone: "warning" };
      }
      return { text: `Add more to your cart to use ${c}`, tone: "warning" };
    }
    case "not_eligible":
      return { text: notEligible(c, code.condition), tone: "warning" };
    case "no_matching_items":
      return { text: `Nothing in your cart qualifies for ${c}`, tone: "warning" };
    case "blocked_by_other": {
      const other = applied.find((a) => a.discount === code.discount)?.title;
      return {
        text: `Can't be combined with ${other ?? "another discount in your cart"}`,
        tone: "warning",
      };
    }
    case "limit_reached":
      return { text: `${c} can't be used any more`, tone: "warning" };
    case "not_available_in_market":
      return { text: `${c} can't be used in this country`, tone: "warning" };
    case "unknown":
      return { text: `${c} doesn't apply to this cart right now`, tone: "warning" };
  }
}

function notEligible(code: string, condition: string): string {
  switch (condition) {
    case "signed_in":
      return `Sign in to use ${code}`;
    case "customer_groups":
    case "customers":
      return `${code} isn't available for your account`;
    case "markets":
      return `${code} can't be used in this country`;
    case "channels":
      return `${code} can't be used here`;
    default:
      return `${code} doesn't apply to this cart right now`;
  }
}

/** The badges a product shows for its promotions: each title once, at most `max`. */
export function promotionBadges(promotions: readonly Promotion[], max = 2): string[] {
  return [...new Set(promotions.map((p) => p.title.trim()).filter(Boolean))].slice(0, max);
}

/** A code as typed, ready to send: trimmed, or `null` when blank. The API normalises
 * case and Unicode; this only keeps a stray space from being a refusal. */
export function typedCode(value: string): string | null {
  const code = value.trim();
  return code === "" ? null : code;
}

/** A shipping offer's amount after a shipping discount (`amount_after_discounts`), or
 * `null` when none applies to it. */
export function readOfferAfter(offer: unknown): Money | null {
  return money(object(offer)?.amount_after_discounts);
}
