// The discount draft and the API's discount, both ways: a draft into the body of
// `POST /admin/discounts` or the JSON Merge Patch of `PATCH /admin/discounts/{id}`, a
// saved discount back into a draft, and the paths of the API's `invalid_discount`
// problems ("kind.rewards[0].value.basis_points") onto the draft's
// ("kind.product.value.percent"). Plain functions only (`node --test`).

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { formatAmount } from "../price-grid/money.ts";
import { fromLocalInput, percentInput, toLocalInput } from "../price-lists.ts";
import {
  type Allocation,
  type ConditionsDraft,
  type DiscountClass,
  type DiscountDraft,
  type ItemSetDraft,
  type KindDraft,
  type PerMarketText,
  type Section,
  type ThresholdDraft,
  type ValueDraft,
  amountMarkets,
  changedSections,
  combinedClasses,
  emptyConditions,
  emptyItems,
  emptyValue,
} from "./draft.ts";
import {
  type CheckContext,
  type MarketOption,
  type PathMapper,
  type Problem,
  checkDraft,
  parseAmounts,
  parseCount,
  parseOff,
  problemsFromDetails,
} from "./validate.ts";

export type Discount = AdminSchemas["DiscountResponse"];
export type CreateDiscount = AdminSchemas["CreateDiscountRequest"];
export type UpdateDiscount = AdminSchemas["UpdateDiscountRequest"];
type Money = AdminSchemas["Money"];
type Kind = AdminSchemas["DiscountKindDto"];
type Condition = AdminSchemas["DiscountConditionDto"];
type Value = AdminSchemas["DiscountValueDto"];
type Items = AdminSchemas["DiscountItemsDto"];
type ItemSet = AdminSchemas["DiscountItemSetDto"];
type Threshold = AdminSchemas["DiscountThresholdDto"];
type Reward = AdminSchemas["DiscountRewardDto"];
type Amounts = Record<string, Money>;

/** A request to send, or the problems that keep the draft from being one. */
export type Built<T> = { body: T } | { problems: Problem[] };

// ---- draft → API ----

/** Money by market, for the markets amounts are typed for; blanks left out. */
function moneyOf(
  text: PerMarketText,
  markets: readonly MarketOption[],
  ctx: CheckContext,
): Amounts {
  const parsed = parseAmounts(text, markets, ctx.currencies, "", { allowZero: true }).amounts;
  const out: Amounts = {};
  for (const market of markets) {
    const amount = parsed[market.id];
    if (amount !== undefined) out[market.id] = { amount, currency: market.currency };
  }
  return out;
}

function percentOf(text: string): number {
  const off = parseOff(text);
  return off.ok ? off.value : 0;
}

function countOf(text: string): number {
  return parseCount(text) ?? 0;
}

/**
 * How an amount is spread: what the draft says for a product reward (`allocation`);
 * `each` for a buy X get Y (an amount off each unit got); left out (across, the only one
 * they take) for the subtotal, shipping and tiers.
 */
type AllocationMode = "draft" | "each" | "default";

function valueOf(
  value: ValueDraft,
  markets: readonly MarketOption[],
  ctx: CheckContext,
  mode: AllocationMode,
): Value {
  switch (value.type) {
    case "percent":
      return { type: "percent", basis_points: percentOf(value.percent) };
    case "fixed_price":
      return { type: "fixed_price", amounts: moneyOf(value.amounts, markets, ctx) };
    case "amount": {
      const amounts = moneyOf(value.amounts, markets, ctx);
      if (mode === "default") return { type: "amount", amounts };
      if (mode === "each") return { type: "amount", amounts, allocation: { type: "each" } };
      const allocation: NonNullable<Extract<Value, { type: "amount" }>["allocation"]> =
        value.allocation === "cheapest"
          ? { type: "cheapest", units: countOf(value.cheapestUnits) }
          : { type: value.allocation };
      return { type: "amount", amounts, allocation };
    }
  }
}

function itemsOf(set: ItemSetDraft): Items {
  switch (set.scope) {
    case "all":
      return { type: "all" };
    case "products":
      return { type: "products", ids: [...set.products] };
    case "variants":
      return { type: "variants", ids: [...set.variants] };
    case "categories":
      return { type: "categories", ids: [...set.categories] };
  }
}

function itemSetOf(set: ItemSetDraft): ItemSet {
  return { items: itemsOf(set), exclude_on_sale: set.excludeOnSale };
}

function thresholdOf(
  threshold: Pick<ThresholdDraft, "quantity" | "amounts">,
  type: "quantity" | "amount",
  markets: readonly MarketOption[],
  ctx: CheckContext,
): Threshold {
  return type === "quantity"
    ? { type: "quantity", quantity: countOf(threshold.quantity) }
    : { type: "amount", amounts: moneyOf(threshold.amounts, markets, ctx) };
}

function kindOf(kind: KindDraft, markets: readonly MarketOption[], ctx: CheckContext): Kind {
  switch (kind.type) {
    case "standard": {
      const rewards: Reward[] = [];
      if (kind.product) {
        rewards.push({
          target: {
            type: "items",
            items: itemsOf(kind.product.items),
            exclude_on_sale: kind.product.items.excludeOnSale,
          },
          value: valueOf(kind.product.value, markets, ctx, "draft"),
        });
      }
      if (kind.order) {
        rewards.push({
          target: { type: "subtotal" },
          value: valueOf(kind.order.value, markets, ctx, "default"),
        });
      }
      if (kind.shipping) {
        const cap = moneyOf(kind.shipping.maxAmount, markets, ctx);
        rewards.push({
          target: { type: "shipping", max_amount: Object.keys(cap).length > 0 ? cap : null },
          value: valueOf(kind.shipping.value, markets, ctx, "default"),
        });
      }
      return { type: "standard", rewards };
    }
    case "buy_x_get_y":
      return {
        type: "buy_x_get_y",
        buys: itemSetOf(kind.buys),
        buy: thresholdOf(kind.buy, kind.buy.type, markets, ctx),
        gets: itemSetOf(kind.gets),
        get_quantity: countOf(kind.getQuantity),
        value: valueOf(kind.value, markets, ctx, "each"),
        uses_per_order: parseCount(kind.usesPerOrder),
      };
    case "tiered":
      return {
        type: "tiered",
        items: itemSetOf(kind.items),
        measure: kind.measure,
        tiers: kind.tiers.map((tier) => ({
          threshold: thresholdOf(
            tier,
            kind.measure === "quantity" ? "quantity" : "amount",
            markets,
            ctx,
          ),
          value: valueOf(tier.value, markets, ctx, "default"),
        })),
      };
  }
}

/** The condition types a draft sends, in the order it sends them. */
type ConditionType = Condition["type"];

function conditionsOf(
  c: ConditionsDraft,
  markets: readonly MarketOption[],
  ctx: CheckContext,
): Condition[] {
  const out: Condition[] = [];
  if (c.markets.length > 0) out.push({ type: "markets", markets: [...c.markets] });
  if (c.channels.length > 0) out.push({ type: "channels", channels: [...c.channels] });
  if (c.customerGroups.length > 0) {
    out.push({ type: "customer_groups", groups: [...c.customerGroups] });
  }
  if (c.customers.length > 0) out.push({ type: "customers", customers: [...c.customers] });
  if (c.signedIn) out.push({ type: "signed_in" });
  if (c.minSubtotal.enabled) {
    out.push({ type: "min_subtotal", amounts: moneyOf(c.minSubtotal.amounts, markets, ctx) });
  }
  if (c.minQuantity.enabled) {
    out.push({ type: "min_quantity", quantity: countOf(c.minQuantity.quantity) });
  }
  return out;
}

/** Every field of a discount a draft sets, as the create request names them. */
function fieldsOf(draft: DiscountDraft, ctx: CheckContext) {
  const markets = amountMarkets(draft.conditions, ctx.markets);
  return {
    title: draft.title.trim(),
    status: draft.status,
    starts_at: draft.schedule.startsAt
      ? fromLocalInput(draft.schedule.startsAt, ctx.timeZone)
      : null,
    ends_at: draft.schedule.endsAt ? fromLocalInput(draft.schedule.endsAt, ctx.timeZone) : null,
    conditions: conditionsOf(draft.conditions, markets, ctx),
    kind: kindOf(draft.kind, markets, ctx),
    combines_with: combinedClasses(draft),
    limits: {
      total: parseCount(draft.limits.totalUses),
      once_per_customer: draft.limits.oncePerCustomer,
    },
  };
}

/** `POST /admin/discounts`'s body for a new draft, its typed code with it. */
export function createBody(draft: DiscountDraft, ctx: CheckContext): Built<CreateDiscount> {
  const problems = checkDraft(draft, ctx);
  if (problems.length > 0) return { problems };
  const code = draft.code.trim();
  return {
    body: {
      ...fieldsOf(draft, ctx),
      method: draft.method,
      ...(draft.method === "codes" && code !== "" ? { codes: [code] } : {}),
    },
  };
}

/**
 * The JSON Merge Patch for what changed between `saved` (the discount as loaded) and
 * `draft`: only the changed sections' fields. Empty when nothing changed. The method and
 * the first code aren't edited here (the method is fixed; codes have their tab).
 */
export function patchBody(
  saved: DiscountDraft,
  draft: DiscountDraft,
  ctx: CheckContext,
): Built<UpdateDiscount> {
  const problems = checkDraft(draft, ctx);
  if (problems.length > 0) return { problems };
  const all = fieldsOf(draft, ctx);
  const body: UpdateDiscount = {};
  const patchers: Partial<Record<Section, () => void>> = {
    title: () => (body.title = all.title),
    status: () => (body.status = all.status),
    kind: () => (body.kind = all.kind),
    conditions: () => (body.conditions = all.conditions),
    combinesWith: () => (body.combines_with = all.combines_with),
    limits: () => (body.limits = all.limits),
    schedule: () => {
      body.starts_at = all.starts_at;
      body.ends_at = all.ends_at;
    },
  };
  for (const section of changedSections(saved, draft)) patchers[section]?.();
  // Amounts are typed for the markets the conditions name: a change of markets changes
  // which amounts the kind sends, so the kind goes along.
  if (body.conditions !== undefined && body.kind === undefined) body.kind = all.kind;
  return { body };
}

// ---- API → draft ----

/** The markets a discount's amounts name, with their currencies: for a store whose
 * markets can't be read, so its amounts still show and are sent back. */
export function marketsInDiscount(discount: Pick<Discount, "kind" | "conditions">): MarketOption[] {
  const found = new Map<string, MarketOption>();
  const add = (amounts: Amounts | null | undefined) => {
    for (const [id, money] of Object.entries(amounts ?? {})) {
      if (!found.has(id)) found.set(id, { id, name: id, currency: money.currency });
    }
  };
  const addValue = (value: Value) => {
    if (value.type !== "percent") add(value.amounts);
  };
  const kind = discount.kind;
  if (kind.type === "standard") {
    for (const reward of kind.rewards) {
      addValue(reward.value);
      if (reward.target.type === "shipping") add(reward.target.max_amount);
    }
  } else if (kind.type === "buy_x_get_y") {
    addValue(kind.value);
    if (kind.buy.type === "amount") add(kind.buy.amounts);
  } else {
    for (const tier of kind.tiers) {
      addValue(tier.value);
      if (tier.threshold.type === "amount") add(tier.threshold.amounts);
    }
  }
  for (const c of discount.conditions) if (c.type === "min_subtotal") add(c.amounts);
  return [...found.values()];
}

/** `markets` plus those only `discount` names (by ID). */
export function withDiscountMarkets(
  markets: readonly MarketOption[],
  discount: Pick<Discount, "kind" | "conditions">,
): MarketOption[] {
  const extra = marketsInDiscount(discount).filter((m) => !markets.some((k) => k.id === m.id));
  return [...markets, ...extra];
}

function textOf(amounts: Amounts | null | undefined, currencies: CheckContext["currencies"]) {
  const out: PerMarketText = {};
  for (const [market, money] of Object.entries(amounts ?? {})) {
    out[market] = formatAmount(money.amount, currencies[money.currency] ?? 2);
  }
  return out;
}

function allocationOf(value: Extract<Value, { type: "amount" }>): {
  allocation: Allocation;
  cheapestUnits: string;
} {
  const allocation = value.allocation;
  if (allocation?.type === "cheapest") {
    return { allocation: "cheapest", cheapestUnits: String(allocation.units) };
  }
  return { allocation: allocation?.type ?? "across", cheapestUnits: "1" };
}

function valueDraft(value: Value, currencies: CheckContext["currencies"]): ValueDraft {
  switch (value.type) {
    case "percent":
      return { ...emptyValue("percent"), percent: percentInput(value.basis_points) };
    case "fixed_price":
      return { ...emptyValue("fixed_price"), amounts: textOf(value.amounts, currencies) };
    case "amount":
      return {
        ...emptyValue("amount"),
        amounts: textOf(value.amounts, currencies),
        ...allocationOf(value),
      };
  }
}

function itemsDraft(items: Items, excludeOnSale: boolean | undefined): ItemSetDraft {
  const set = { ...emptyItems(items.type), excludeOnSale: excludeOnSale ?? false };
  if (items.type !== "all") set[items.type] = [...items.ids];
  return set;
}

function thresholdDraft(
  threshold: Threshold,
  currencies: CheckContext["currencies"],
): ThresholdDraft {
  return threshold.type === "quantity"
    ? { type: "quantity", quantity: String(threshold.quantity), amounts: {} }
    : { type: "amount", quantity: "", amounts: textOf(threshold.amounts, currencies) };
}

function kindDraft(kind: Kind, currencies: CheckContext["currencies"]): KindDraft {
  switch (kind.type) {
    case "standard": {
      const draft: KindDraft = { type: "standard", product: null, order: null, shipping: null };
      for (const reward of kind.rewards) {
        const value = valueDraft(reward.value, currencies);
        const target = reward.target;
        if (target.type === "items") {
          draft.product = { items: itemsDraft(target.items, target.exclude_on_sale), value };
        } else if (target.type === "subtotal") {
          draft.order = { value };
        } else {
          draft.shipping = { value, maxAmount: textOf(target.max_amount, currencies) };
        }
      }
      return draft;
    }
    case "buy_x_get_y":
      return {
        type: "buy_x_get_y",
        buys: itemsDraft(kind.buys.items, kind.buys.exclude_on_sale),
        buy: thresholdDraft(kind.buy, currencies),
        gets: itemsDraft(kind.gets.items, kind.gets.exclude_on_sale),
        getQuantity: String(kind.get_quantity),
        value: valueDraft(kind.value, currencies),
        usesPerOrder: kind.uses_per_order == null ? "" : String(kind.uses_per_order),
      };
    case "tiered": {
      const measure = kind.measure === "subtotal" ? "subtotal" : "quantity";
      return {
        type: "tiered",
        items: itemsDraft(kind.items.items, kind.items.exclude_on_sale),
        measure,
        tiers: kind.tiers.map((tier) => {
          const threshold = thresholdDraft(tier.threshold, currencies);
          return {
            quantity: threshold.quantity,
            amounts: threshold.amounts,
            value: valueDraft(tier.value, currencies),
          };
        }),
      };
    }
  }
}

function conditionsDraft(
  conditions: readonly Condition[],
  currencies: CheckContext["currencies"],
): ConditionsDraft {
  const draft = emptyConditions();
  for (const c of conditions) {
    switch (c.type) {
      case "markets":
        draft.markets = [...c.markets];
        break;
      case "channels":
        draft.channels = [...c.channels];
        break;
      case "customer_groups":
        draft.customerGroups = [...c.groups];
        break;
      case "customers":
        draft.customers = [...c.customers];
        break;
      case "signed_in":
        draft.signedIn = true;
        break;
      case "min_subtotal":
        draft.minSubtotal = { enabled: true, amounts: textOf(c.amounts, currencies) };
        break;
      case "min_quantity":
        draft.minQuantity = { enabled: true, quantity: String(c.quantity) };
        break;
    }
  }
  return draft;
}

const isClass = (c: string): c is DiscountClass =>
  c === "product" || c === "order" || c === "shipping";

/** A saved discount as the form edits it. */
export function draftOf(
  discount: Pick<
    Discount,
    | "method"
    | "title"
    | "status"
    | "kind"
    | "conditions"
    | "combines_with"
    | "limits"
    | "starts_at"
    | "ends_at"
  >,
  ctx: Pick<CheckContext, "currencies" | "timeZone">,
): DiscountDraft {
  const combines = new Set(discount.combines_with.filter(isClass));
  return {
    method: discount.method === "automatic" ? "automatic" : "codes",
    title: discount.title,
    code: "",
    status: discount.status === "draft" ? "draft" : "active",
    kind: kindDraft(discount.kind, ctx.currencies),
    conditions: conditionsDraft(discount.conditions, ctx.currencies),
    combinesWith: {
      product: combines.has("product"),
      order: combines.has("order"),
      shipping: combines.has("shipping"),
    },
    limits: {
      totalUses: discount.limits.total == null ? "" : String(discount.limits.total),
      oncePerCustomer: discount.limits.once_per_customer ?? false,
    },
    schedule: {
      startsAt: toLocalInput(discount.starts_at, ctx.timeZone),
      endsAt: toLocalInput(discount.ends_at, ctx.timeZone),
    },
  };
}

// ---- the API's problem paths → the draft's ----

/** "kind.rewards[0].value" → ["kind", "rewards", 0, "value"]; JSON pointers and "$." too. */
export function segments(path: string): (string | number)[] {
  return path
    .replace(/^\$\.?/, "")
    .replace(/^\//, "")
    .replaceAll("/", ".")
    .replaceAll(/\[(\d+)\]/g, ".$1")
    .split(".")
    .filter((s) => s !== "")
    .map((s) => (/^\d+$/.test(s) ? Number(s) : s));
}

type Rest = readonly (string | number)[];

/** A market ID after an amounts path, when the API named one. */
const marketAfter = (base: string, market: string | number | undefined) =>
  typeof market === "string" ? `${base}.${market}` : base;

function valuePath(base: string, rest: Rest): string {
  switch (rest[0]) {
    case "basis_points":
      return `${base}.percent`;
    case "amounts":
      return marketAfter(`${base}.amounts`, rest[1]);
    case "allocation":
      return rest[1] === "units" ? `${base}.cheapestUnits` : `${base}.type`;
    default:
      return `${base}.type`;
  }
}

/** The field showing an item set's chosen IDs (or its scope, for every product). */
function itemsPath(base: string, set: ItemSetDraft): string {
  return set.scope === "all" ? `${base}.scope` : `${base}.${set.scope}`;
}

/** A path inside an `ItemSet` (`{items, exclude_on_sale}`). */
function itemSetPath(base: string, set: ItemSetDraft, rest: Rest): string {
  return rest[0] === "exclude_on_sale" ? `${base}.excludeOnSale` : itemsPath(base, set);
}

function kindPath(kind: KindDraft, rest: Rest): string {
  if (rest.length === 0) return "kind";
  switch (kind.type) {
    case "standard": {
      if (rest[0] !== "rewards" || typeof rest[1] !== "number") return "kind";
      const cls = rewardClasses(kind)[rest[1]];
      if (!cls) return "kind";
      const [part, ...inner] = rest.slice(2);
      if (part === "value") return valuePath(`kind.${cls}.value`, inner);
      if (part !== "target") return "kind";
      if (cls === "product" && kind.product) {
        return inner[0] === "exclude_on_sale"
          ? "kind.product.items.excludeOnSale"
          : inner[0] === "items"
            ? itemsPath("kind.product.items", kind.product.items)
            : "kind";
      }
      if (cls === "shipping" && inner[0] === "max_amount") {
        return marketAfter("kind.shipping.maxAmount", inner[1]);
      }
      return "kind";
    }
    case "buy_x_get_y":
      switch (rest[0]) {
        case "buys":
          return itemSetPath("kind.buys", kind.buys, rest.slice(1));
        case "gets":
          return itemSetPath("kind.gets", kind.gets, rest.slice(1));
        case "buy":
          if (rest[1] === "amounts") return marketAfter("kind.buy.amounts", rest[2]);
          return kind.buy.type === "quantity" ? "kind.buy.quantity" : "kind.buy.amounts";
        case "get_quantity":
          return "kind.getQuantity";
        case "value":
          return valuePath("kind.value", rest.slice(1));
        case "uses_per_order":
          return "kind.usesPerOrder";
        default:
          return "kind";
      }
    case "tiered": {
      if (rest[0] === "items") return itemSetPath("kind.items", kind.items, rest.slice(1));
      if (rest[0] !== "tiers") return "kind";
      const i = rest[1];
      if (typeof i !== "number" || i >= kind.tiers.length) return "kind.tiers";
      const base = `kind.tiers.${i}`;
      if (rest[2] === "value") return valuePath(`${base}.value`, rest.slice(3));
      if (kind.measure === "quantity") return `${base}.quantity`;
      return rest[3] === "amounts" ? marketAfter(`${base}.amounts`, rest[4]) : `${base}.amounts`;
    }
  }
}

// What a request's shape needs of the context: with no markets to type amounts for,
// nothing reads the currencies or the time zone.
const SHAPE_ONLY: CheckContext = { markets: [], currencies: {}, timeZone: "UTC" };

/** The condition types `draft` sends, in the order `conditionsOf` sends them. */
function conditionTypes(draft: DiscountDraft): ConditionType[] {
  return conditionsOf(draft.conditions, [], SHAPE_ONLY).map((c) => c.type);
}

const REWARD_CLASSES: Record<Reward["target"]["type"], DiscountClass> = {
  items: "product",
  subtotal: "order",
  shipping: "shipping",
};

/** The rewards' classes in the order `kindOf` sends them. */
function rewardClasses(kind: KindDraft): DiscountClass[] {
  const sent = kindOf(kind, [], SHAPE_ONLY);
  return sent.type === "standard" ? sent.rewards.map((r) => REWARD_CLASSES[r.target.type]) : [];
}

const CONDITION_FIELDS: Record<ConditionType, string> = {
  markets: "conditions.markets",
  channels: "conditions.channels",
  customer_groups: "conditions.customerGroups",
  customers: "conditions.customers",
  signed_in: "conditions.signedIn",
  min_subtotal: "conditions.minSubtotal.amounts",
  min_quantity: "conditions.minQuantity.quantity",
};

/**
 * Where the API's paths land in `draft`, the draft the request was built from: rewards
 * and conditions are found by their place in the request, as `createBody` and
 * `patchBody` order them. A path the form has no field for is null.
 */
export function apiPathMapper(draft: DiscountDraft): PathMapper {
  const conditions = conditionTypes(draft);
  return (apiPath) => {
    const [head, ...rest] = segments(apiPath);
    switch (head) {
      case "title":
      case "status":
      case "method":
        return head;
      case "codes":
        return "code";
      case "starts_at":
        return "schedule.startsAt";
      case "ends_at":
        return "schedule.endsAt";
      case "combines_with":
        return "combinesWith";
      case "limits":
        return rest[0] === "once_per_customer" ? "limits.oncePerCustomer" : "limits.totalUses";
      case "conditions": {
        const i = rest[0];
        const type = typeof i === "number" ? conditions[i] : undefined;
        if (!type) return null;
        const field = CONDITION_FIELDS[type];
        return type === "min_subtotal" ? marketAfter(field, rest[2]) : field;
      }
      case "kind":
        return kindPath(draft.kind, rest);
      default:
        return null;
    }
  };
}

// ---- labels ----

const count = (n: number) => n.toLocaleString("en");

/** "Automatic", "1 code", "2,000 codes". */
export function methodLabel(d: Pick<Discount, "method" | "code_count">): string {
  if (d.method === "automatic") return "Automatic";
  return d.code_count === 1 ? "1 code" : `${count(d.code_count)} codes`;
}

/** "12", or "12 of 100" with a limit. */
export function usesLabel(d: Pick<Discount, "used" | "limits">): string {
  return d.limits.total == null ? count(d.used) : `${count(d.used)} of ${count(d.limits.total)}`;
}

// ---- refusals ----

/** A refused write as the server action hands it back: the API's code, a message for
 * the person (`describe`) and the API's `details`. */
export type Refusal = { code: string; message: string; details?: unknown };

function detail(details: unknown, key: string): string | undefined {
  if (typeof details !== "object" || details === null) return undefined;
  const value: unknown = Reflect.get(details, key);
  return typeof value === "string" ? value : undefined;
}

/**
 * The problems a refused create or update shows, on the draft's fields where the API
 * says which (`invalid_discount`'s problems, a market an amount names, a code taken),
 * else under "" (the whole form).
 */
export function refusalProblems(refusal: Refusal, draft: DiscountDraft): Problem[] {
  const map = apiPathMapper(draft);
  const path = detail(refusal.details, "path");
  const market = detail(refusal.details, "market");
  const at = (message: string): Problem[] => {
    const field = path ? map(market ? `${path}.${market}` : path) : null;
    return [{ path: field ?? "", message }];
  };
  switch (refusal.code) {
    case "invalid_discount":
      return problemsFromDetails(refusal.details, map);
    case "market_not_found":
      return at("This market no longer exists: clear its amount.");
    case "discount_currency_mismatch":
      return at("This market sells in another currency now: type its amount again.");
    case "discount_code_taken": {
      const code = detail(refusal.details, "code");
      const message = code ? `Another discount has the code ${code}.` : "Another discount has it.";
      return [{ path: "code", message }];
    }
    case "invalid_discount_code":
      return [{ path: "code", message: "1 to 64 characters, without spaces." }];
    case "too_many_automatic_discounts":
      return [
        {
          path: "",
          message:
            "The store has as many live automatic discounts as it can hold. End or pause one first, or save this one as a draft.",
        },
      ];
    default:
      return [{ path: "", message: refusal.message }];
  }
}
