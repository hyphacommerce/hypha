// Discounts as shoppers read them: a cart's codes and what each one's state means, the
// discounts applied with what they took off each line and the shipping, the totals'
// "Discounts" row, "Free" shipping and the promotion badges on products. Pure, so
// servers and browsers share it.
//
// The `read*` functions map the API's answers onto what the pages show.

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { translated } from "./catalog-languages.ts";
import type { TFor } from "./messages.ts";
import { type Money, formatMoney } from "./money.ts";
import { lineCompareAt } from "./prices.ts";

/** Why a code the cart keeps does or doesn't apply now. */
export type CodeState =
  | { state: "limit_reached" }
  /** It would leave the cart costing no less, say by losing free shipping. */
  | { state: "no_saving" }
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
  /** Its discount ended. */
  | { state: "expired" }
  /** The code or its discount went: deleted, deactivated or made a draft. */
  | { state: "invalid" }
  /** A state this storefront doesn't know yet. */
  | { state: "unknown" };

/** A code on the cart (normalised, as the API keeps it) and its state. */
export type CartCode = { code: string } & CodeState;

/** One discount applied: its title and summary for the buyer, what it took off, and the
 * reward it gave (the API's value and target), to word it in other languages. */
export type Application = {
  index: number;
  discount: string;
  code: string | null;
  title: string;
  /** The API's sentence, in English. */
  summary: string;
  value: Schemas["DiscountValueDto"];
  target: Schemas["DiscountTargetDto"];
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
  /** Whether there's a code field to offer: a cart's, not an order's. */
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

/** A product's live automatic discount, shown as a badge, never as a price. `summary` is
 * the API's sentence, in English. */
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

type Schemas = StorefrontSchemas;
type Share = Schemas["DiscountShareResponse"];
type CodeResponse = Schemas["CartDiscountCodeResponse"];
type AppliedResponse = Pick<
  Schemas["CartDiscountResponse"],
  "discount" | "code" | "title" | "summary" | "value" | "target" | "amount"
>;

/** What reading a cart needs of it: a priced line has `subtotal` (before discounts),
 * `total` (after) and `discounts`; an unavailable line has none of them. */
export type CartFields = Pick<Schemas["CartResponse"], "discount_codes" | "discount_total"> & {
  discounts: readonly AppliedResponse[];
  lines: readonly { variant: string; subtotal?: Money; total?: Money; discounts?: Share[] }[];
  shipping_groups: readonly {
    id: string;
    selected?: Pick<Schemas["SelectedShippingResponse"], "amount" | "total" | "discounts"> | null;
  }[];
};

/** What reading an order needs of it. */
export type OrderFields = Pick<Schemas["StorefrontOrderResponse"], "discount_total"> & {
  discounts: readonly AppliedResponse[];
  lines: readonly Pick<
    Schemas["StorefrontOrderLineResponse"],
    "id" | "subtotal" | "total" | "discounts"
  >[];
  shipping_lines: readonly Pick<
    Schemas["StorefrontShippingLineResponse"],
    "group" | "amount" | "total" | "discounts"
  >[];
};

/** What came off shipping across the shipments, `null` when nothing did. */
function shippingOff(shipments: Record<string, Shipment>): Money | null {
  const all = Object.values(shipments);
  if (all.length === 0) return null;
  const amount = all.reduce((n, s) => n + s.before.amount - s.after.amount, 0);
  return { amount, currency: all[0].before.currency };
}

function allocations(shares: readonly Share[]): Allocation[] {
  return shares.map((s) => ({ application: s.application, amount: s.amount }));
}

/** A code's state, as the API sends it. A state this storefront doesn't know is
 * `unknown`: the API may add some, and an unknown one means "not applied". */
export function readCodeState(entry: Omit<CodeResponse, "code">): CodeState {
  switch (entry.state) {
    case "applied":
    case "no_matching_items":
    case "limit_reached":
    case "no_saving":
    case "not_available_in_market":
    case "expired":
    case "invalid":
      return { state: entry.state };
    case "not_eligible":
      return { state: "not_eligible", condition: entry.condition ?? "" };
    case "minimum_not_met":
      return {
        state: "minimum_not_met",
        needed: {
          subtotal: entry.needed?.subtotal ?? undefined,
          quantity: entry.needed?.quantity ?? undefined,
        },
      };
    case "blocked_by_other":
      return { state: "blocked_by_other", discount: entry.discount ?? "" };
    default:
      return { state: "unknown" };
  }
}

/** The applications, numbered by their place in the list: what lines and shipping name. */
function applications(discounts: readonly AppliedResponse[]): Application[] {
  return discounts.map((a, index) => ({
    index,
    discount: a.discount,
    code: a.code ?? null,
    title: a.title,
    summary: a.summary,
    value: a.value,
    target: a.target,
    amount: a.amount,
  }));
}

/** What a cart says about discounts. A line or a shipment counts as discounted only
 * when a discount took something off it. */
export function readCartDiscounts(cart: CartFields | null | undefined): Discounts {
  if (!cart) return NO_DISCOUNTS;
  const lines: Record<string, LineDiscount> = {};
  for (const line of cart.lines) {
    const taken = allocations(line.discounts ?? []);
    if (line.subtotal && line.total && taken.length > 0) {
      lines[line.variant] = { before: line.subtotal, after: line.total, allocations: taken };
    }
  }
  const shipments: Record<string, Shipment> = {};
  for (const group of cart.shipping_groups) {
    const s = group.selected;
    if (s && s.discounts.length > 0) shipments[group.id] = { before: s.amount, after: s.total };
  }
  return {
    takesCodes: true,
    codes: cart.discount_codes.map((entry) => ({ code: entry.code, ...readCodeState(entry) })),
    applications: applications(cart.discounts),
    total: cart.discount_total.amount > 0 ? cart.discount_total : null,
    shipping: shippingOff(shipments),
    lines,
    shipments,
  };
}

/** What a placed order says: its applications, each line's `subtotal` (before) and
 * `total` (after), each shipping line's `amount` and `total`. */
export function readOrderDiscounts(order: OrderFields | null | undefined): Discounts {
  if (!order) return NO_DISCOUNTS;
  const lines: Record<string, LineDiscount> = {};
  for (const line of order.lines) {
    const taken = allocations(line.discounts);
    if (taken.length > 0) {
      lines[line.id] = { before: line.subtotal, after: line.total, allocations: taken };
    }
  }
  const shipments: Record<string, Shipment> = {};
  for (const s of order.shipping_lines) {
    if (s.discounts.length > 0) shipments[s.group] = { before: s.amount, after: s.total };
  }
  return {
    takesCodes: false,
    codes: [],
    applications: applications(order.discounts),
    total: order.discount_total.amount > 0 ? order.discount_total : null,
    shipping: shippingOff(shipments),
    lines,
    shipments,
  };
}

/** A product's (or a search hit's) `promotions`: empty without a country. */
export function readPromotions(product: {
  promotions?: readonly Schemas["PromotionResponse"][];
}): Promotion[] {
  return (product.promotions ?? []).map((p) => ({ title: p.title, summary: p.summary }));
}

// ---- What's shown ---------------------------------------------------------------------

/** "−€2.40": an amount taken off (a minus sign, not a hyphen). */
export function minus(amount: Money, digits: number, locale: string): string {
  return `−${formatMoney(amount, digits, locale)}`;
}

/** What a line shows: its total after discounts, the total before struck through, and a
 * note per discount ("−€2.40 Welcome offer"). Without discounts, `price` is `total`. */
export function lineView(
  t: TFor<"discounts">,
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
      const title =
        applied.find((x) => x.index === a.application)?.title ?? t("discounts.fallback_title");
      return t("discounts.note", { amount: minus(a.amount, digits, t.locale), title });
    });
  return { price: line.after, struck: line.before, notes };
}

/** A cart or order line's price as shown: after discounts, struck through beside the
 * total before them, else beside its compare-at total (a sale) as before. */
export function linePrice(
  t: TFor<"discounts">,
  line: { total: Money; quantity: number; compare_at_price?: Money | null },
  discount: LineDiscount | undefined,
  applied: Application[],
  digits: number,
): { price: Money; compareAt: Money | null; notes: string[] } {
  const view = lineView(t, line.total, discount, applied, digits);
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

/** The cart drawer's rows, reading as the cart page's totals do: "Subtotal" is before
 * discounts, then "Discounts" and "Subtotal after discounts" when something came off the
 * goods. `subtotal` is after discounts, as the API sends it. */
export function subtotalRows(
  t: TFor<"cart">,
  subtotal: Money,
  discounts: Pick<Discounts, "total" | "shipping">,
  digits: number,
): { label: string; value: string; strong: boolean }[] {
  const shown = discountedTotals(subtotal, discounts);
  const money = (amount: Money) => formatMoney(amount, digits, t.locale);
  if (!shown.goods) {
    return [{ label: t("cart.subtotal"), value: money(subtotal), strong: true }];
  }
  return [
    { label: t("cart.subtotal"), value: money(shown.subtotal), strong: false },
    { label: t("cart.discounts"), value: minus(shown.goods, digits, t.locale), strong: false },
    { label: t("cart.subtotal_after_discounts"), value: money(subtotal), strong: true },
  ];
}

/** A shipping price: "Free" when a discount took all of it, else the amount, with the
 * price before a discount struck through. A rate that's free anyway stays "€0.00". */
export function shippingPrice(
  t: TFor<"cart">,
  amount: Money,
  before: Money | null | undefined,
  digits: number,
): { text: string; was: string | null } {
  const discounted = before && before.amount > amount.amount ? before : null;
  return {
    text:
      discounted && amount.amount <= 0
        ? t("cart.shipping_free")
        : formatMoney(amount, digits, t.locale),
    was: discounted ? formatMoney(discounted, digits, t.locale) : null,
  };
}

const warning = (text: string) => ({ text, tone: "warning" as const });

/** What a code's state tells the shopper, and whether it's good news. */
export function codeMessage(
  t: TFor<"discounts">,
  code: CartCode,
  applied: Application[],
  digits: number,
): { text: string; tone: "success" | "warning" } {
  const c = code.code;
  switch (code.state) {
    case "applied": {
      const title = applied.find((a) => a.code === c)?.title;
      return {
        text: title ? t("discounts.applied_title", { title }) : t("discounts.applied"),
        tone: "success",
      };
    }
    case "minimum_not_met": {
      const { subtotal, quantity } = code.needed;
      if (subtotal && subtotal.amount > 0) {
        const amount = formatMoney(subtotal, digits, t.locale);
        return warning(t("discounts.add_amount", { amount, code: c }));
      }
      if (quantity && quantity > 0) {
        return warning(t("discounts.add_items", { count: quantity, code: c }));
      }
      return warning(t("discounts.add_more", { code: c }));
    }
    case "not_eligible":
      return warning(notEligible(t, c, code.condition));
    case "no_matching_items":
      return warning(t("discounts.nothing_qualifies", { code: c }));
    case "blocked_by_other": {
      const other = applied.find((a) => a.discount === code.discount)?.title;
      return warning(
        other ? t("discounts.cant_combine", { other }) : t("discounts.cant_combine_another"),
      );
    }
    case "limit_reached":
      return warning(t("discounts.limit_reached", { code: c }));
    case "no_saving":
      return warning(t("discounts.no_saving", { code: c }));
    case "not_available_in_market":
      return warning(t("discounts.not_in_country", { code: c }));
    case "expired":
      return warning(t("discounts.expired", { code: c }));
    case "invalid":
      return warning(t("discounts.invalid", { code: c }));
    case "unknown":
      return warning(t("discounts.not_applicable", { code: c }));
  }
}

function notEligible(t: TFor<"discounts">, code: string, condition: string): string {
  switch (condition) {
    case "signed_in":
      return t("discounts.sign_in", { code });
    case "customer_groups":
    case "customers":
      return t("discounts.not_for_account", { code });
    case "markets":
      return t("discounts.not_in_country", { code });
    case "channels":
      return t("discounts.not_here", { code });
    default:
      return t("discounts.not_applicable", { code });
  }
}

/** Whether a page in `language` reads the English catalog: the API's sentences (a
 * discount's `summary`) are English, so only such a page shows them as they are. */
function readsEnglish(language: string): boolean {
  return translated(language) === undefined;
}

/** The one amount `amounts` (by market) has in `currency`, or `null` when there's none
 * or its markets differ: the sentence then says nothing it can't be sure of. */
function amountIn(amounts: Readonly<Record<string, Money>>, currency: string): Money | null {
  const found = new Set(
    Object.values(amounts)
      .filter((m) => m.currency === currency)
      .map((m) => m.amount),
  );
  const [only] = found;
  return found.size === 1 && only !== undefined ? { amount: only, currency } : null;
}

/**
 * What an applied discount gave, as a sentence in the page's language: the API's
 * `summary` on an English page, else one built from its reward where that reads simply
 * (off the order, off shipping), else `null`, leaving its title to say it. A reward on
 * items is never worded: a "buy X get Y" or a tier reads as a plain percentage off them.
 */
export function discountSentence(
  t: TFor<"discounts">,
  a: Pick<Application, "summary" | "value" | "target" | "amount">,
  digits: number,
): string | null {
  if (readsEnglish(t.language)) return a.summary === "" ? null : a.summary;
  const currency = a.amount.currency;
  const money = (m: Money) => formatMoney(m, digits, t.locale);
  const { value, target } = a;
  const percent =
    value.type === "percent"
      ? new Intl.NumberFormat(t.locale, { style: "percent", maximumFractionDigits: 2 }).format(
          value.basis_points / 10_000,
        )
      : null;
  const amount = value.type === "amount" ? amountIn(value.amounts, currency) : null;
  switch (target.type) {
    case "subtotal":
      if (percent) return t("discounts.sentence_order_percent", { percent });
      if (amount) return t("discounts.sentence_order_amount", { amount: money(amount) });
      return null;
    case "shipping": {
      const max = target.max_amount;
      const cap = max ? amountIn(max, currency) : null;
      if (max && !cap) return null;
      if (value.type === "percent" && value.basis_points >= 10_000) {
        return cap
          ? t("discounts.sentence_shipping_free_up_to", { cap: money(cap) })
          : t("discounts.sentence_shipping_free");
      }
      if (percent) {
        return cap
          ? t("discounts.sentence_shipping_percent_up_to", { percent, cap: money(cap) })
          : t("discounts.sentence_shipping_percent", { percent });
      }
      if (amount) return t("discounts.sentence_shipping_amount", { amount: money(amount) });
      return null;
    }
    default:
      return null;
  }
}

/** The promotions whose summary a product shows under its price: those with one, on an
 * English page; none elsewhere, where the badges (titles the API translated) say it and
 * a promotion carries no data to word it with. */
export function summarisedPromotions(
  t: TFor<"discounts">,
  promotions: readonly Promotion[],
): Promotion[] {
  return readsEnglish(t.language) ? promotions.filter((p) => p.summary) : [];
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
