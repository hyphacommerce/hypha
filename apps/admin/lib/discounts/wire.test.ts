import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type ConditionsDraft,
  type DiscountDraft,
  type KindDraft,
  type ValueDraft,
  emptyConditions,
  emptyDraft,
  emptyItems,
  emptyValue,
} from "./draft.ts";
import type { CheckContext } from "./validate.ts";
import {
  type CreateDiscount,
  type Discount,
  apiPathMapper,
  changedSections,
  createBody,
  draftOf,
  isDirty,
  marketsInDiscount,
  patchBody,
  refusalProblems,
  segments,
  withDiscountMarkets,
} from "./wire.ts";

const ctx: CheckContext = {
  markets: [
    { id: "mkt_ie", name: "Ireland", currency: "EUR" },
    { id: "mkt_jp", name: "Japan", currency: "JPY" },
  ],
  currencies: { EUR: 2, JPY: 0 },
  timeZone: "Europe/Dublin",
};

const eur = (amount: number) => ({ amount, currency: "EUR" });
const jpy = (amount: number) => ({ amount, currency: "JPY" });

function draft(change: Partial<DiscountDraft> = {}): DiscountDraft {
  return { ...emptyDraft(), title: "Welcome offer", ...change };
}

function percent(text: string): ValueDraft {
  return { ...emptyValue("percent"), percent: text };
}

function amounts(type: ValueDraft["type"], ie: string, jp?: string): ValueDraft {
  return { ...emptyValue(type), amounts: jp ? { mkt_ie: ie, mkt_jp: jp } : { mkt_ie: ie } };
}

function standard(change: Partial<Extract<KindDraft, { type: "standard" }>>): KindDraft {
  return { type: "standard", product: null, order: null, shipping: null, ...change };
}

function body(d: DiscountDraft): CreateDiscount {
  const built = createBody(d, ctx);
  if ("problems" in built) assert.fail(JSON.stringify(built.problems));
  return built.body;
}

/** The draft the API's answer to `d` reads back as: the create body as a response. */
function roundTrip(d: DiscountDraft): DiscountDraft {
  const sent = body(d);
  const saved: Discount = {
    id: "disc_1",
    phase: "active",
    classes: [],
    summary: "",
    code_count: 0,
    used: 0,
    discounted_total: [],
    created_at: "2026-10-24T00:00:00Z",
    updated_at: "2026-10-24T00:00:00Z",
    title: sent.title,
    method: sent.method,
    status: sent.status ?? "active",
    starts_at: sent.starts_at ?? null,
    ends_at: sent.ends_at ?? null,
    conditions: sent.conditions ?? [],
    kind: sent.kind,
    combines_with: sent.combines_with ?? [],
    limits: sent.limits ?? {},
  };
  return draftOf(saved, ctx);
}

/** A draft as it comes back: the first code isn't part of a saved discount. */
const saved = (d: DiscountDraft): DiscountDraft => ({ ...d, code: "" });

// ---- the body ----

test("a new code discount sends its code, method and every field", () => {
  const d = draft({ code: " WELCOME10 " });
  assert.deepEqual(body(d), {
    title: "Welcome offer",
    status: "active",
    starts_at: null,
    ends_at: null,
    conditions: [],
    kind: {
      type: "standard",
      rewards: [
        {
          target: { type: "items", items: { type: "all" }, exclude_on_sale: false },
          value: { type: "percent", basis_points: 1000 },
        },
      ],
    },
    combines_with: [],
    limits: { total: null, once_per_customer: false },
    method: "codes",
    codes: ["WELCOME10"],
  });
});

test("a blank code and an automatic discount send no codes", () => {
  assert.equal(body(draft({ code: "" })).codes, undefined);
  assert.equal(body(draft({ method: "automatic", code: "IGNORED" })).codes, undefined);
});

test("a draft that breaks a rule is its problems, not a body", () => {
  const built = createBody(draft({ title: " " }), ctx);
  assert.deepEqual("problems" in built && built.problems.map((p) => p.path), ["title"]);
});

test("rewards go products, order, shipping; only items take an allocation", () => {
  const product = { ...amounts("amount", "2.50", "300"), allocation: "cheapest" as const };
  const d = draft({
    kind: standard({
      product: { items: { ...emptyItems("categories"), categories: ["pcat_1"] }, value: product },
      order: { value: { ...amounts("amount", "5"), allocation: "each" } },
      shipping: { value: percent("100"), maxAmount: { mkt_ie: "10", mkt_jp: "" } },
    }),
  });
  assert.deepEqual(body(d).kind, {
    type: "standard",
    rewards: [
      {
        target: {
          type: "items",
          items: { type: "categories", ids: ["pcat_1"] },
          exclude_on_sale: false,
        },
        value: {
          type: "amount",
          amounts: { mkt_ie: eur(250), mkt_jp: jpy(300) },
          allocation: { type: "cheapest", units: 1 },
        },
      },
      { target: { type: "subtotal" }, value: { type: "amount", amounts: { mkt_ie: eur(500) } } },
      {
        target: { type: "shipping", max_amount: { mkt_ie: eur(1000) } },
        value: { type: "percent", basis_points: 10_000 },
      },
    ],
  });
});

test("shipping without a cap sends max_amount null", () => {
  const d = draft({ kind: standard({ shipping: { value: percent("100"), maxAmount: {} } }) });
  const kind = body(d).kind;
  assert.deepEqual(kind.type === "standard" && kind.rewards[0]?.target, {
    type: "shipping",
    max_amount: null,
  });
});

test("amounts are sent only for the markets the conditions name", () => {
  const d = draft({
    conditions: { ...emptyConditions(), markets: ["mkt_jp"] },
    kind: standard({ order: { value: amounts("amount", "5", "500") } }),
  });
  const kind = body(d).kind;
  assert.deepEqual(kind.type === "standard" && kind.rewards[0]?.value, {
    type: "amount",
    amounts: { mkt_jp: jpy(500) },
  });
});

test("buy X get Y sends its sides, threshold and an amount off each unit", () => {
  const d = draft({
    kind: {
      type: "buy_x_get_y",
      buys: { ...emptyItems("products"), products: ["prod_kit"], excludeOnSale: true },
      buy: { type: "amount", quantity: "", amounts: { mkt_ie: "30" } },
      gets: { ...emptyItems("variants"), variants: ["var_syringe"] },
      getQuantity: "2",
      value: { ...amounts("amount", "1"), allocation: "across" },
      usesPerOrder: "",
    },
  });
  assert.deepEqual(body(d).kind, {
    type: "buy_x_get_y",
    buys: { items: { type: "products", ids: ["prod_kit"] }, exclude_on_sale: true },
    buy: { type: "amount", amounts: { mkt_ie: eur(3000) } },
    gets: { items: { type: "variants", ids: ["var_syringe"] }, exclude_on_sale: false },
    get_quantity: 2,
    value: { type: "amount", amounts: { mkt_ie: eur(100) }, allocation: { type: "each" } },
    uses_per_order: null,
  });
});

test("tiers send their thresholds by the measure", () => {
  const d = draft({
    kind: {
      type: "tiered",
      items: emptyItems(),
      measure: "subtotal",
      tiers: [
        { quantity: "", amounts: { mkt_ie: "50" }, value: percent("5") },
        { quantity: "", amounts: { mkt_ie: "100" }, value: amounts("amount", "15") },
      ],
    },
  });
  assert.deepEqual(body(d).kind, {
    type: "tiered",
    items: { items: { type: "all" }, exclude_on_sale: false },
    measure: "subtotal",
    tiers: [
      {
        threshold: { type: "amount", amounts: { mkt_ie: eur(5000) } },
        value: { type: "percent", basis_points: 500 },
      },
      {
        threshold: { type: "amount", amounts: { mkt_ie: eur(10_000) } },
        value: { type: "amount", amounts: { mkt_ie: eur(1500) } },
      },
    ],
  });
});

test("conditions go in a fixed order, only those set", () => {
  const conditions: ConditionsDraft = {
    markets: ["mkt_ie"],
    channels: ["chan_1"],
    customerGroups: ["cgrp_1"],
    customers: ["cus_1"],
    signedIn: true,
    minSubtotal: { enabled: true, amounts: { mkt_ie: "50" } },
    minQuantity: { enabled: true, quantity: "3" },
  };
  assert.deepEqual(body(draft({ conditions })).conditions, [
    { type: "markets", markets: ["mkt_ie"] },
    { type: "channels", channels: ["chan_1"] },
    { type: "customer_groups", groups: ["cgrp_1"] },
    { type: "customers", customers: ["cus_1"] },
    { type: "signed_in" },
    { type: "min_subtotal", amounts: { mkt_ie: eur(5000) } },
    { type: "min_quantity", quantity: 3 },
  ]);
});

test("the schedule is sent in UTC from the store's wall-clock times", () => {
  const sent = body(draft({ schedule: { startsAt: "2026-11-27T00:00", endsAt: "" } }));
  assert.deepEqual([sent.starts_at, sent.ends_at], ["2026-11-27T00:00:00Z", null]);
  const summer = body(draft({ schedule: { startsAt: "", endsAt: "2026-07-01T09:30" } }));
  assert.equal(summer.ends_at, "2026-07-01T08:30:00Z");
});

test("limits and combinations", () => {
  const sent = body(
    draft({
      limits: { totalUses: "100", oncePerCustomer: true },
      combinesWith: { product: false, order: true, shipping: true },
    }),
  );
  assert.deepEqual(
    [sent.limits, sent.combines_with],
    [{ total: 100, once_per_customer: true }, ["order", "shipping"]],
  );
});

// ---- round trips ----

const ROUND_TRIPS: Record<string, DiscountDraft> = {
  "percent off every product": draft(),
  "percent off chosen products, not on sale": draft({
    kind: standard({
      product: {
        items: { ...emptyItems("products"), products: ["prod_1", "prod_2"], excludeOnSale: true },
        value: percent("12.5"),
      },
    }),
  }),
  "an amount off each variant": draft({
    kind: standard({
      product: {
        items: { ...emptyItems("variants"), variants: ["var_1"] },
        value: { ...amounts("amount", "2.40", "300"), allocation: "each" },
      },
    }),
  }),
  "an amount off the 2 cheapest in categories": draft({
    kind: standard({
      product: {
        items: { ...emptyItems("categories"), categories: ["pcat_1"] },
        value: { ...amounts("amount", "5.00"), allocation: "cheapest", cheapestUnits: "2" },
      },
    }),
  }),
  "a fixed price, free allowed": draft({
    kind: standard({
      product: { items: emptyItems(), value: amounts("fixed_price", "0.00", "1000") },
    }),
  }),
  "an amount off the order": draft({
    kind: standard({ order: { value: amounts("amount", "10.00") } }),
  }),
  "free shipping up to a cap, with an order reward": draft({
    kind: standard({
      order: { value: percent("10") },
      shipping: { value: percent("100"), maxAmount: { mkt_ie: "10.00" } },
    }),
  }),
  "an amount off shipping": draft({
    kind: standard({ shipping: { value: amounts("amount", "3.50"), maxAmount: {} } }),
  }),
  "buy 2 get 1 free, once per order": draft({
    method: "automatic",
    kind: {
      type: "buy_x_get_y",
      buys: { ...emptyItems("products"), products: ["prod_kit"] },
      buy: { type: "quantity", quantity: "2", amounts: {} },
      gets: { ...emptyItems("categories"), categories: ["pcat_syringes"], excludeOnSale: true },
      getQuantity: "1",
      value: percent("100"),
      usesPerOrder: "1",
    },
  }),
  "spend to get an amount off each": draft({
    kind: {
      type: "buy_x_get_y",
      buys: emptyItems(),
      buy: { type: "amount", quantity: "", amounts: { mkt_ie: "30.00", mkt_jp: "4000" } },
      gets: emptyItems(),
      getQuantity: "3",
      value: { ...amounts("amount", "1.00"), allocation: "each" },
      usesPerOrder: "",
    },
  }),
  "quantity tiers": draft({
    kind: {
      type: "tiered",
      items: { ...emptyItems("categories"), categories: ["pcat_cultures"] },
      measure: "quantity",
      tiers: [
        { quantity: "3", amounts: {}, value: percent("10") },
        { quantity: "5", amounts: {}, value: percent("15") },
      ],
    },
  }),
  "spend tiers": draft({
    kind: {
      type: "tiered",
      items: emptyItems(),
      measure: "subtotal",
      tiers: [
        { quantity: "", amounts: { mkt_ie: "50.00" }, value: amounts("amount", "5.00") },
        { quantity: "", amounts: { mkt_ie: "100.00" }, value: amounts("amount", "15.00") },
      ],
    },
  }),
  "every condition, limits, combinations and a window": draft({
    status: "draft",
    conditions: {
      markets: ["mkt_ie"],
      channels: ["chan_1"],
      customerGroups: ["cgrp_1"],
      customers: ["cus_1", "cus_2"],
      signedIn: true,
      minSubtotal: { enabled: true, amounts: { mkt_ie: "50.00" } },
      minQuantity: { enabled: true, quantity: "2" },
    },
    combinesWith: { product: true, order: false, shipping: true },
    limits: { totalUses: "500", oncePerCustomer: true },
    schedule: { startsAt: "2026-11-27T00:00", endsAt: "2026-12-01T00:00" },
  }),
};

for (const [name, d] of Object.entries(ROUND_TRIPS)) {
  test(`round trip: ${name}`, () => {
    assert.deepEqual(roundTrip(d), saved(d));
  });
}

test("an amount answered without an allocation reads as across", () => {
  const d = roundTrip(draft({ kind: standard({ order: { value: amounts("amount", "1.00") } }) }));
  assert.equal(d.kind.type === "standard" && d.kind.order?.value.allocation, "across");
});

test("amounts show with their currency's digits", () => {
  const d = roundTrip(
    draft({ kind: standard({ order: { value: amounts("amount", "5", "500") } }) }),
  );
  assert.deepEqual(d.kind.type === "standard" && d.kind.order?.value.amounts, {
    mkt_ie: "5.00",
    mkt_jp: "500",
  });
});

// ---- the patch ----

test("a patch holds only the changed sections", () => {
  const before = saved(draft());
  const after = {
    ...before,
    title: "Autumn offer",
    limits: { totalUses: "", oncePerCustomer: true },
  };
  const built = patchBody(before, after, ctx);
  assert.deepEqual("body" in built && built.body, {
    title: "Autumn offer",
    limits: { total: null, once_per_customer: true },
  });
});

test("an unchanged draft patches nothing", () => {
  const before = saved(draft());
  assert.deepEqual(patchBody(before, structuredClone(before), ctx), { body: {} });
});

test("a schedule change sends both ends, null for a cleared one", () => {
  const before = saved(draft({ schedule: { startsAt: "2026-11-27T00:00", endsAt: "" } }));
  const after = { ...before, schedule: { startsAt: "", endsAt: "2026-12-01T00:00" } };
  const built = patchBody(before, after, ctx);
  assert.deepEqual("body" in built && built.body, {
    starts_at: null,
    ends_at: "2026-12-01T00:00:00Z",
  });
});

test("changed conditions send the kind too, its amounts following the markets", () => {
  const before = saved(
    draft({ kind: standard({ order: { value: amounts("amount", "5", "500") } }) }),
  );
  const after = { ...before, conditions: { ...emptyConditions(), markets: ["mkt_ie"] } };
  const built = patchBody(before, after, ctx);
  assert.deepEqual("body" in built && built.body, {
    conditions: [{ type: "markets", markets: ["mkt_ie"] }],
    kind: {
      type: "standard",
      rewards: [
        { target: { type: "subtotal" }, value: { type: "amount", amounts: { mkt_ie: eur(500) } } },
      ],
    },
  });
});

test("a patch of a broken draft is its problems", () => {
  const before = saved(draft());
  const built = patchBody(before, { ...before, title: "" }, ctx);
  assert.ok("problems" in built);
});

// ---- markets only the discount names ----

test("markets the discount names but the store list lacks are added by ID", () => {
  const sent = body(draft({ kind: standard({ order: { value: amounts("amount", "5") } }) }));
  const kind = { ...sent.kind };
  if (kind.type === "standard") {
    kind.rewards = [
      {
        target: { type: "subtotal" },
        value: { type: "amount", amounts: { mkt_us: { amount: 5, currency: "USD" } } },
      },
    ];
  }
  const discount = {
    kind,
    conditions: [{ type: "min_subtotal" as const, amounts: { mkt_ie: eur(1) } }],
  };
  assert.deepEqual(marketsInDiscount(discount), [
    { id: "mkt_us", name: "mkt_us", currency: "USD" },
    { id: "mkt_ie", name: "mkt_ie", currency: "EUR" },
  ]);
  assert.deepEqual(
    withDiscountMarkets(ctx.markets, discount).map((m) => m.id),
    ["mkt_ie", "mkt_jp", "mkt_us"],
  );
});

// ---- the API's paths ----

test("segments read dotted, indexed and pointer paths", () => {
  assert.deepEqual(segments("kind.rewards[0].value.basis_points"), [
    "kind",
    "rewards",
    0,
    "value",
    "basis_points",
  ]);
  assert.deepEqual(segments("/limits/total"), ["limits", "total"]);
  assert.deepEqual(segments("$.codes[2]"), ["codes", 2]);
});

test("the route test's paths land on the draft's fields", () => {
  const map = apiPathMapper(draft({ kind: standard({ order: { value: percent("10") } }) }));
  assert.deepEqual(["title", "limits.total", "kind.rewards[0].value.basis_points"].map(map), [
    "title",
    "limits.totalUses",
    "kind.order.value.percent",
  ]);
});

test("rewards are found by their place in the request", () => {
  const map = apiPathMapper(
    draft({
      kind: standard({
        product: { items: { ...emptyItems("products"), products: ["p"] }, value: percent("10") },
        shipping: { value: amounts("amount", "1"), maxAmount: {} },
      }),
    }),
  );
  assert.deepEqual(
    [
      "kind.rewards[0].target.items.ids[1]",
      "kind.rewards[0].target.exclude_on_sale",
      "kind.rewards[0].value.allocation.units",
      "kind.rewards[0].value.allocation",
      "kind.rewards[1].value.amounts.mkt_ie",
      "kind.rewards[1].value.amounts",
      "kind.rewards[1].target.max_amount.mkt_jp",
      "kind.rewards[1].target",
      "kind.rewards[5].value",
      "kind.rewards",
    ].map(map),
    [
      "kind.product.items.products",
      "kind.product.items.excludeOnSale",
      "kind.product.value.cheapestUnits",
      "kind.product.value.type",
      "kind.shipping.value.amounts.mkt_ie",
      "kind.shipping.value.amounts",
      "kind.shipping.maxAmount.mkt_jp",
      "kind",
      "kind",
      "kind",
    ],
  );
});

test("conditions are found by their place in the request", () => {
  const map = apiPathMapper(
    draft({
      conditions: {
        ...emptyConditions(),
        customers: ["cus_1"],
        minSubtotal: { enabled: true, amounts: {} },
        minQuantity: { enabled: true, quantity: "0" },
      },
    }),
  );
  assert.deepEqual(
    [
      "conditions[0].customers[0]",
      "conditions[1].amounts",
      "conditions[1].amounts.mkt_ie",
      "conditions[2].quantity",
      "conditions[3]",
    ].map(map),
    [
      "conditions.customers",
      "conditions.minSubtotal.amounts",
      "conditions.minSubtotal.amounts.mkt_ie",
      "conditions.minQuantity.quantity",
      null,
    ],
  );
});

test("every condition's problem lands on its field, in the order the request sends them", () => {
  const d = draft({
    kind: standard({ order: { value: percent("10") } }),
    conditions: {
      ...emptyConditions(),
      markets: ["mkt_ie"],
      channels: ["chan_1"],
      customerGroups: ["cgrp_1"],
      customers: ["cus_1"],
      signedIn: true,
      minSubtotal: { enabled: true, amounts: { mkt_ie: "10" } },
      minQuantity: { enabled: true, quantity: "2" },
    },
  });
  const field: Record<string, string> = {
    markets: "conditions.markets",
    channels: "conditions.channels",
    customer_groups: "conditions.customerGroups",
    customers: "conditions.customers",
    signed_in: "conditions.signedIn",
    min_subtotal: "conditions.minSubtotal.amounts",
    min_quantity: "conditions.minQuantity.quantity",
  };
  const sent = body(d).conditions ?? [];
  assert.equal(sent.length, Object.keys(field).length);
  const map = apiPathMapper(d);
  sent.forEach((c, i) => assert.equal(map(`conditions[${i}]`), field[c.type], c.type));
});

test("every reward's problem lands on its class, in the order the request sends them", () => {
  const d = draft({
    kind: standard({
      product: { items: emptyItems(), value: percent("10") },
      order: { value: percent("5") },
      shipping: { value: percent("100"), maxAmount: {} },
    }),
  });
  const kind = body(d).kind;
  assert.equal(kind.type, "standard");
  if (kind.type !== "standard") return;
  const cls = { items: "product", subtotal: "order", shipping: "shipping" } as const;
  const map = apiPathMapper(d);
  kind.rewards.forEach((r, i) =>
    assert.equal(
      map(`kind.rewards[${i}].value.basis_points`),
      `kind.${cls[r.target.type]}.value.percent`,
    ),
  );
});

// ---- what changed ----

const orderOff = (text: string) =>
  draft({ kind: standard({ order: { value: amounts("amount", text) } }) });

/** 10% off every product, with `products` left from when it named some. */
const everyProduct = (products: string[]) =>
  draft({
    kind: standard({
      product: { items: { ...emptyItems("all"), products }, value: percent("10") },
    }),
  });

const tiered = (first: string, second: string) =>
  draft({
    kind: {
      type: "tiered",
      items: emptyItems(),
      measure: "quantity",
      tiers: [first, second].map((quantity) => ({ quantity, amounts: {}, value: percent("5") })),
    },
  });

test("an unchanged draft has no changed sections", () => {
  assert.deepEqual(changedSections(orderOff("12.50"), orderOff("12.50"), ctx), []);
  assert.equal(isDirty(orderOff("12.50"), orderOff("12.50"), ctx), false);
});

test("changed sections are only those that differ", () => {
  const edited = {
    ...orderOff("12.50"),
    title: "Welcome",
    limits: { totalUses: "5", oncePerCustomer: false },
  };
  assert.deepEqual(changedSections(orderOff("12.50"), edited, ctx), ["title", "limits"]);
});

test("an amount typed another way isn't a change", () => {
  assert.deepEqual(changedSections(orderOff("12.50"), orderOff("12.5"), ctx), []);
  assert.deepEqual(changedSections(orderOff("12.50"), orderOff("12.51"), ctx), ["kind"]);
});

test("a product list hidden behind Every product isn't a change", () => {
  assert.deepEqual(changedSections(everyProduct([]), everyProduct(["prod_1"]), ctx), []);
});

test("the order of IDs in a list isn't a change", () => {
  const before = orderOff("12.50");
  before.conditions.markets = ["mkt_ie", "mkt_jp"];
  const edited = orderOff("12.50");
  edited.conditions.markets = ["mkt_jp", "mkt_ie"];
  assert.deepEqual(changedSections(before, edited, ctx), []);
});

test("a blank amount is the same as none", () => {
  const before = orderOff("12.50");
  before.conditions.minSubtotal = { enabled: true, amounts: { mkt_ie: "10" } };
  const edited = orderOff("12.50");
  edited.conditions.minSubtotal = { enabled: true, amounts: { mkt_ie: "10", mkt_jp: " " } };
  assert.deepEqual(changedSections(before, edited, ctx), []);
});

test("a draft with problems is compared by what was typed", () => {
  const broken = { ...orderOff("12.50"), title: "" };
  assert.deepEqual(changedSections(orderOff("12.5"), broken, ctx), ["title", "kind"]);
});

test("reordering tiers is a change", () => {
  assert.deepEqual(changedSections(tiered("3", "5"), tiered("5", "3"), ctx), ["kind"]);
});

test("buy X get Y paths", () => {
  const map = apiPathMapper(
    draft({
      kind: {
        type: "buy_x_get_y",
        buys: emptyItems(),
        buy: { type: "amount", quantity: "", amounts: {} },
        gets: { ...emptyItems("categories"), categories: [] },
        getQuantity: "0",
        value: percent("100"),
        usesPerOrder: "0",
      },
    }),
  );
  assert.deepEqual(
    [
      "kind.buys.items",
      "kind.gets.items.ids",
      "kind.gets.exclude_on_sale",
      "kind.buy.amounts",
      "kind.buy.amounts.mkt_ie",
      "kind.get_quantity",
      "kind.value.basis_points",
      "kind.value.allocation",
      "kind.uses_per_order",
    ].map(map),
    [
      "kind.buys.scope",
      "kind.gets.categories",
      "kind.gets.excludeOnSale",
      "kind.buy.amounts",
      "kind.buy.amounts.mkt_ie",
      "kind.getQuantity",
      "kind.value.percent",
      "kind.value.type",
      "kind.usesPerOrder",
    ],
  );
});

test("tier paths, by the measure", () => {
  const tiers = [
    { quantity: "3", amounts: {}, value: percent("10") },
    { quantity: "2", amounts: {}, value: percent("15") },
  ];
  const quantity = apiPathMapper(
    draft({ kind: { type: "tiered", items: emptyItems(), measure: "quantity", tiers } }),
  );
  assert.deepEqual(
    [
      "kind.tiers[1].threshold",
      "kind.tiers[1].threshold.quantity",
      "kind.tiers[0].value.basis_points",
      "kind.tiers",
      "kind.tiers[9]",
    ].map(quantity),
    [
      "kind.tiers.1.quantity",
      "kind.tiers.1.quantity",
      "kind.tiers.0.value.percent",
      "kind.tiers",
      "kind.tiers",
    ],
  );
  const spend = apiPathMapper(
    draft({ kind: { type: "tiered", items: emptyItems(), measure: "subtotal", tiers } }),
  );
  assert.deepEqual(
    [
      "kind.tiers[1].threshold",
      "kind.tiers[1].threshold.amounts.mkt_ie",
      "kind.items.items.ids",
    ].map(spend),
    ["kind.tiers.1.amounts", "kind.tiers.1.amounts.mkt_ie", "kind.items.scope"],
  );
});

test("plain fields and what the form has no field for", () => {
  const map = apiPathMapper(draft());
  assert.deepEqual(
    [
      "codes[0]",
      "starts_at",
      "ends_at",
      "combines_with",
      "limits.once_per_customer",
      "nothing",
    ].map(map),
    [
      "code",
      "schedule.startsAt",
      "schedule.endsAt",
      "combinesWith",
      "limits.oncePerCustomer",
      null,
    ],
  );
});

// ---- refusals ----

test("invalid_discount's problems land on their fields, the rest on the form", () => {
  const d = draft({ kind: standard({ order: { value: percent("10") } }) });
  const problems = refusalProblems(
    {
      code: "invalid_discount",
      message: "the discount breaks 2 rule(s)",
      details: {
        problems: [
          {
            path: "kind.rewards[0].value.basis_points",
            code: "out_of_range",
            message: "1 to 10000 basis points",
          },
          { path: "somewhere.new", code: "x", message: "something else" },
        ],
      },
    },
    d,
  );
  assert.deepEqual(problems, [
    { path: "kind.order.value.percent", message: "1 to 10000 basis points." },
    { path: "", message: "Something else." },
  ]);
});

test("a market an amount names is the amount's field", () => {
  const d = draft({
    conditions: { ...emptyConditions(), minSubtotal: { enabled: true, amounts: {} } },
  });
  assert.deepEqual(
    refusalProblems(
      {
        code: "market_not_found",
        message: "",
        details: { path: "conditions[0].amounts", market: "mkt_x" },
      },
      d,
    ).map((p) => p.path),
    ["conditions.minSubtotal.amounts.mkt_x"],
  );
});

test("a taken code is the code's field, naming it", () => {
  assert.deepEqual(
    refusalProblems(
      { code: "discount_code_taken", message: "", details: { code: "WELCOME10" } },
      draft(),
    ),
    [{ path: "code", message: "Another discount has the code WELCOME10." }],
  );
});

test("anything else is the whole form's, in the person's words", () => {
  assert.deepEqual(refusalProblems({ code: "missing_scope", message: "You can't." }, draft()), [
    { path: "", message: "You can't." },
  ]);
});
