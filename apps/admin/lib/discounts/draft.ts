// A discount as the admin's form holds it: what staff type (amounts and percentages as
// text, times as wall-clock times in the store's zone), shaped by the form's sections
// rather than by the API's body. Turning it into a request is the page's job; here are
// the draft, its blank form, the classes it carries and what changed since it was saved.
// Plain functions and types only, shared by server and client code (and `node --test`).

/** How buyers get it: a code they enter, or applied by itself. */
export type Method = "codes" | "automatic";

/** Combination categories. A discount has one per reward; `combines_with` names them too. */
export type DiscountClass = "product" | "order" | "shipping";

export const CLASSES: readonly DiscountClass[] = ["product", "order", "shipping"];

/** `draft` never applies; `active` applies inside its window. */
export type Status = "draft" | "active";

/** What the list shows: the status with the window taken into account. */
export type Phase = "draft" | "scheduled" | "active" | "expired";

/**
 * How an amount off items is spread: off each unit, once across the items (split by
 * their amounts), or off the cheapest units.
 */
export type Allocation = "each" | "across" | "cheapest";

/** Amounts as typed, by market ID ("12.50"). A market left blank isn't offered it. */
export type PerMarketText = Record<string, string>;

/**
 * A reward's value. `percent` is the reduction as typed ("20" is 20% off, "100" free);
 * `amounts` serve both an amount off and a fixed price; `allocation` and `cheapestUnits`
 * matter only for an amount off items.
 */
export type ValueDraft = {
  type: "percent" | "amount" | "fixed_price";
  percent: string;
  amounts: PerMarketText;
  allocation: Allocation;
  /** How many of the cheapest units, for `cheapest`. */
  cheapestUnits: string;
};

export type ItemsScope = "all" | "products" | "variants" | "categories";

/** What a reward or a buy X get Y side covers. Categories count their descendants. */
export type ItemSetDraft = {
  scope: ItemsScope;
  products: string[];
  variants: string[];
  categories: string[];
  /** "Not on items already reduced" by a sale price list. */
  excludeOnSale: boolean;
};

export type ProductRewardDraft = { items: ItemSetDraft; value: ValueDraft };
export type OrderRewardDraft = { value: ValueDraft };
export type ShippingRewardDraft = {
  value: ValueDraft;
  /** "Free shipping up to €10": the most taken off, by market; blank for no cap. */
  maxAmount: PerMarketText;
};

/** One reward per class: a standard discount has at least one of them. */
export type StandardDraft = {
  type: "standard";
  product: ProductRewardDraft | null;
  order: OrderRewardDraft | null;
  shipping: ShippingRewardDraft | null;
};

export type ThresholdDraft = {
  type: "quantity" | "amount";
  quantity: string;
  amounts: PerMarketText;
};

/** Buy X get Y. The form shows it read-only until the API has it. */
export type BuyXGetYDraft = {
  type: "buy_x_get_y";
  buys: ItemSetDraft;
  buy: ThresholdDraft;
  gets: ItemSetDraft;
  getQuantity: string;
  /** A percentage ("100" is free) or an amount off each unit got. */
  value: ValueDraft;
  /** Blank: as often as the cart qualifies. */
  usesPerOrder: string;
};

export type TierDraft = {
  /** A quantity for `quantity`, else blank. */
  quantity: string;
  /** A subtotal by market for `subtotal`, else empty. */
  amounts: PerMarketText;
  value: ValueDraft;
};

/** Volume tiers. The form shows it read-only until the API has it. */
export type TieredDraft = {
  type: "tiered";
  items: ItemSetDraft;
  measure: "quantity" | "subtotal";
  tiers: TierDraft[];
};

export type KindDraft = StandardDraft | BuyXGetYDraft | TieredDraft;

/** Who and which carts it's for: all must hold, a list matches any of its values. */
export type ConditionsDraft = {
  markets: string[];
  channels: string[];
  customerGroups: string[];
  customers: string[];
  signedIn: boolean;
  minSubtotal: { enabled: boolean; amounts: PerMarketText };
  minQuantity: { enabled: boolean; quantity: string };
};

export type DiscountDraft = {
  method: Method;
  /** Shown to buyers ("Welcome offer"). */
  title: string;
  /** A codes discount's first code; blank to add or generate codes later. */
  code: string;
  status: Status;
  kind: KindDraft;
  conditions: ConditionsDraft;
  combinesWith: Record<DiscountClass, boolean>;
  limits: {
    /** Blank: unlimited. */
    totalUses: string;
    oncePerCustomer: boolean;
  };
  /** Wall-clock times in the store's zone (`2026-11-27T00:00`); blank for none. */
  schedule: { startsAt: string; endsAt: string };
};

/** The most IDs a condition or an item set lists. */
export const MAX_IDS = 1000;
/** The most tiers a volume discount has. */
export const MAX_TIERS = 10;
/** The longest title. */
export const MAX_TITLE = 255;
/** The longest code. */
export const MAX_CODE = 64;

export function emptyValue(type: ValueDraft["type"] = "percent"): ValueDraft {
  return { type, percent: "", amounts: {}, allocation: "across", cheapestUnits: "1" };
}

export function emptyItems(scope: ItemsScope = "all"): ItemSetDraft {
  return { scope, products: [], variants: [], categories: [], excludeOnSale: false };
}

export function emptyConditions(): ConditionsDraft {
  return {
    markets: [],
    channels: [],
    customerGroups: [],
    customers: [],
    signedIn: false,
    minSubtotal: { enabled: false, amounts: {} },
    minQuantity: { enabled: false, quantity: "" },
  };
}

/** A new discount: a code, active at once, 10% off every product, combining with nothing. */
export function emptyDraft(): DiscountDraft {
  return {
    method: "codes",
    title: "",
    code: "",
    status: "active",
    kind: {
      type: "standard",
      product: { items: emptyItems(), value: { ...emptyValue(), percent: "10" } },
      order: null,
      shipping: null,
    },
    conditions: emptyConditions(),
    combinesWith: { product: false, order: false, shipping: false },
    limits: { totalUses: "", oncePerCustomer: false },
    schedule: { startsAt: "", endsAt: "" },
  };
}

/** The classes a draft carries: one per standard reward; buy X get Y and tiers are product. */
export function classesOf(kind: KindDraft): DiscountClass[] {
  if (kind.type !== "standard") return ["product"];
  return CLASSES.filter((c) => kind[c] !== null);
}

/** The classes it combines with, in their fixed order. */
export function combinedClasses(draft: Pick<DiscountDraft, "combinesWith">): DiscountClass[] {
  return CLASSES.filter((c) => draft.combinesWith[c]);
}

export type ClassSettings = {
  classes: readonly DiscountClass[];
  combinesWith: readonly DiscountClass[];
};

/**
 * Whether two discounts' class settings let them apply together: each must combine with
 * every class of the other.
 */
export function compatible(a: ClassSettings, b: ClassSettings): boolean {
  return (
    b.classes.every((c) => a.combinesWith.includes(c)) &&
    a.classes.every((c) => b.combinesWith.includes(c))
  );
}

/** The phase a status and window (RFC 3339) come to at `now` (ms), as the list shows it. */
export function phaseOf(
  status: string,
  startsAt: string | null | undefined,
  endsAt: string | null | undefined,
  now: number,
): Phase {
  if (status === "draft") return "draft";
  if (startsAt && Date.parse(startsAt) > now) return "scheduled";
  if (endsAt && Date.parse(endsAt) <= now) return "expired";
  return "active";
}

/** The draft's sections, the units a save sends when they changed. */
export type Section = keyof DiscountDraft;

const SECTIONS: readonly Section[] = [
  "method",
  "title",
  "code",
  "status",
  "kind",
  "conditions",
  "combinesWith",
  "limits",
  "schedule",
];

/** Keys whose arrays are ordered: tiers go from the lowest threshold up. */
const ORDERED = new Set(["tiers"]);
/** Keys holding amounts by market, where a blank amount is no amount. */
const AMOUNT_MAPS = new Set(["amounts", "maxAmount"]);

/**
 * A canonical form for comparing: ID lists sorted (their order means nothing), blank
 * amounts dropped (a blank market isn't offered it either way), text trimmed.
 */
function canonical(value: unknown, key: string): unknown {
  if (Array.isArray(value)) {
    const items: unknown[] = value.map((v) => canonical(v, ""));
    return !ORDERED.has(key) && items.every((v) => typeof v === "string")
      ? items.toSorted()
      : items;
  }
  if (typeof value === "object" && value !== null) {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value).toSorted(([a], [b]) => a.localeCompare(b))) {
      if (AMOUNT_MAPS.has(key) && typeof v === "string" && v.trim() === "") continue;
      out[k] = canonical(v, k);
    }
    return out;
  }
  return typeof value === "string" ? value.trim() : value;
}

/**
 * The sections of `draft` that differ from `saved`, for a JSON Merge Patch of changed
 * fields only. Empty when nothing changed.
 */
export function changedSections(saved: DiscountDraft, draft: DiscountDraft): Section[] {
  return SECTIONS.filter(
    (s) => JSON.stringify(canonical(saved[s], s)) !== JSON.stringify(canonical(draft[s], s)),
  );
}

export function isDirty(saved: DiscountDraft, draft: DiscountDraft): boolean {
  return changedSections(saved, draft).length > 0;
}

/**
 * The markets a draft's amounts are typed for: those its markets condition names, or
 * every market when it names none.
 */
export function amountMarkets<M extends { id: string }>(
  conditions: Pick<ConditionsDraft, "markets">,
  markets: readonly M[],
): M[] {
  if (conditions.markets.length === 0) return [...markets];
  return markets.filter((m) => conditions.markets.includes(m.id));
}
