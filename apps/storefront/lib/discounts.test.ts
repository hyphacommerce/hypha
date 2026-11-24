import { createTranslator } from "@hyphacommerce/i18n";

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  type Application,
  type CartCode,
  type CartFields,
  NO_DISCOUNTS,
  codeMessage,
  discountSentence,
  discountedTotals,
  linePrice,
  lineView,
  minus,
  promotionBadges,
  readCartDiscounts,
  readCodeState,
  readOrderDiscounts,
  readPromotions,
  shippingPrice,
  subtotalRows,
  summarisedPromotions,
  typedCode,
} from "./discounts.ts";
import type { Money } from "./money.ts";
import { en, loadMessages } from "./messages.ts";

/** English, formatted for `en`: what the expected strings are written in. */
const t = createTranslator("en", en);
const sr = createTranslator("sr-Latn", await loadMessages("sr-Latn"), { fallback: en });
/** A page in a language without a catalog: it reads English. */
const de = createTranslator("de", en);

const eur = (amount: number) => ({ amount, currency: "EUR" });
const welcome: Application = {
  index: 0,
  discount: "disc_1",
  code: "WELCOME10",
  title: "Welcome offer",
  summary: "10% off everything",
  value: { type: "percent", basis_points: 1000 },
  target: { type: "items", items: { type: "all" } },
  amount: eur(240),
};
const freeShipping: Application = {
  index: 1,
  discount: "disc_2",
  code: null,
  title: "Free shipping over €50",
  summary: "Free shipping on orders over €50",
  value: { type: "percent", basis_points: 10_000 },
  target: { type: "shipping", max_amount: null },
  amount: eur(500),
};
const message = (code: CartCode, applied: Application[] = []) =>
  codeMessage(t, code, applied, 2).text;
const emptyCart: CartFields = {
  discount_codes: [],
  discounts: [],
  discount_total: eur(0),
  lines: [],
  shipping_groups: [],
};

test("a cart without discounts has none, but takes codes", () => {
  assert.deepEqual(
    readCartDiscounts({
      ...emptyCart,
      lines: [{ variant: "var_1", subtotal: eur(100), total: eur(100), discounts: [] }],
    }),
    { ...NO_DISCOUNTS, takesCodes: true },
  );
  assert.deepEqual(readCartDiscounts(null), NO_DISCOUNTS);
});

test("a cart's codes, applications, lines and shipping are read", () => {
  const read = readCartDiscounts({
    discount_codes: [
      { code: "WELCOME10", state: "applied" },
      { code: "BIG", state: "minimum_not_met", needed: { subtotal: eur(1200) } },
    ],
    discounts: [welcome, freeShipping],
    discount_total: eur(740),
    lines: [
      {
        variant: "var_1",
        subtotal: eur(2400),
        total: eur(2160),
        discounts: [{ application: 0, amount: eur(240) }],
      },
      // An unavailable line has no prices.
      { variant: "var_2" },
    ],
    shipping_groups: [
      {
        id: "grp_1",
        selected: {
          amount: eur(500),
          total: eur(0),
          discounts: [{ application: 1, amount: eur(500) }],
        },
      },
    ],
  });
  assert.deepEqual(read.codes, [
    { code: "WELCOME10", state: "applied" },
    {
      code: "BIG",
      state: "minimum_not_met",
      needed: { subtotal: eur(1200), quantity: undefined },
    },
  ]);
  assert.equal(read.takesCodes, true);
  assert.deepEqual(read.applications, [welcome, freeShipping]);
  assert.deepEqual(read.total, eur(740));
  assert.deepEqual(read.shipping, eur(500));
  assert.deepEqual(read.lines, {
    var_1: {
      before: eur(2400),
      after: eur(2160),
      allocations: [{ application: 0, amount: eur(240) }],
    },
  });
  assert.deepEqual(read.shipments.grp_1, { before: eur(500), after: eur(0) });
});

test("each known code state is read with its fields", () => {
  assert.deepEqual(readCodeState({ state: "expired" }), { state: "expired" });
  assert.deepEqual(readCodeState({ state: "invalid" }), { state: "invalid" });
  assert.deepEqual(readCodeState({ state: "not_eligible", condition: "signed_in" }), {
    state: "not_eligible",
    condition: "signed_in",
  });
  assert.deepEqual(readCodeState({ state: "blocked_by_other", discount: "disc_2" }), {
    state: "blocked_by_other",
    discount: "disc_2",
  });
  assert.deepEqual(readCodeState({ state: "minimum_not_met", needed: { quantity: 2 } }), {
    state: "minimum_not_met",
    needed: { subtotal: undefined, quantity: 2 },
  });
});

test("an unknown code state is kept as unknown", () => {
  assert.deepEqual(readCodeState({ state: "something_new" }), { state: "unknown" });
});

test("an order's lines are keyed by ID, before being the subtotal", () => {
  const read = readOrderDiscounts({
    discounts: [welcome],
    discount_total: eur(240),
    lines: [
      {
        id: "line_1",
        subtotal: eur(2400),
        total: eur(2160),
        discounts: [{ application: 0, amount: eur(240) }],
      },
      { id: "line_2", subtotal: eur(500), total: eur(500), discounts: [] },
    ],
    shipping_lines: [{ group: "grp_1", amount: eur(500), total: eur(500), discounts: [] }],
  });
  assert.deepEqual(Object.keys(read.lines), ["line_1"]);
  assert.equal(read.shipping, null);
  assert.deepEqual(read.applications, [welcome]);
});

test("an order without discounts reads as none", () => {
  assert.deepEqual(
    readOrderDiscounts({
      discounts: [],
      discount_total: eur(0),
      lines: [{ id: "line_1", subtotal: eur(100), total: eur(100), discounts: [] }],
      shipping_lines: [],
    }),
    NO_DISCOUNTS,
  );
});

test("a discounted line shows its total after, the original struck and a note per discount", () => {
  const line = {
    before: eur(2400),
    after: eur(2160),
    allocations: [{ application: 0, amount: eur(240) }],
  };
  assert.deepEqual(lineView(t, eur(2400), line, [welcome], 2), {
    price: eur(2160),
    struck: eur(2400),
    notes: ["−€2.40 Welcome offer"],
  });
});

test("a line without discounts shows its total alone", () => {
  assert.deepEqual(lineView(t, eur(2400), undefined, [], 2), {
    price: eur(2400),
    struck: null,
    notes: [],
  });
});

test("a zero allocation adds no note", () => {
  const line = {
    before: eur(1000),
    after: eur(900),
    allocations: [
      { application: 0, amount: eur(100) },
      { application: 1, amount: eur(0) },
    ],
  };
  assert.deepEqual(lineView(t, eur(1000), line, [welcome, freeShipping], 2).notes, [
    "−€1.00 Welcome offer",
  ]);
});

test("a discounted line strikes its total before discounts, not its compare-at", () => {
  const line = { total: eur(2400), quantity: 2, compare_at_price: eur(1500) };
  const discount = {
    before: eur(2400),
    after: eur(2160),
    allocations: [{ application: 0, amount: eur(240) }],
  };
  assert.deepEqual(linePrice(t, line, discount, [welcome], 2), {
    price: eur(2160),
    compareAt: eur(2400),
    notes: ["−€2.40 Welcome offer"],
  });
});

test("a line on sale without discounts strikes its compare-at total", () => {
  const line = { total: eur(2400), quantity: 2, compare_at_price: eur(1500) };
  assert.deepEqual(linePrice(t, line, undefined, [], 2), {
    price: eur(2400),
    compareAt: eur(3000),
    notes: [],
  });
});

test("minus uses a minus sign and the currency's digits", () => {
  assert.equal(minus(eur(240), 2, "en"), "−€2.40");
  assert.equal(minus({ amount: 240, currency: "JPY" }, 0, "en"), "−¥240");
});

test("the Discounts row is what came off the goods, the subtotal shown before it", () => {
  assert.deepEqual(discountedTotals(eur(2160), { total: eur(740), shipping: eur(500) }), {
    subtotal: eur(2400),
    goods: eur(240),
  });
});

test("without discounts on goods there's no Discounts row", () => {
  assert.deepEqual(discountedTotals(eur(2400), { total: eur(500), shipping: eur(500) }), {
    subtotal: eur(2400),
    goods: null,
  });
  assert.deepEqual(discountedTotals(eur(2400), NO_DISCOUNTS), {
    subtotal: eur(2400),
    goods: null,
  });
});

test("the drawer's subtotal is before discounts, as the cart page's", () => {
  assert.deepEqual(subtotalRows(t, eur(2160), { total: eur(740), shipping: eur(500) }, 2), [
    { label: "Subtotal", value: "€24.00", strong: false },
    { label: "Discounts", value: "−€2.40", strong: false },
    { label: "Subtotal after discounts", value: "€21.60", strong: true },
  ]);
});

test("without discounts on goods the drawer shows one subtotal", () => {
  assert.deepEqual(subtotalRows(t, eur(2400), { total: eur(500), shipping: eur(500) }, 2), [
    { label: "Subtotal", value: "€24.00", strong: true },
  ]);
});

test("shipping a discount took all of is Free, the price struck", () => {
  assert.deepEqual(shippingPrice(t, eur(0), eur(500), 2), { text: "Free", was: "€5.00" });
});

test("shipping partly discounted shows both amounts", () => {
  assert.deepEqual(shippingPrice(t, eur(200), eur(500), 2), { text: "€2.00", was: "€5.00" });
});

test("a rate that's free anyway isn't called Free", () => {
  assert.deepEqual(shippingPrice(t, eur(0), null, 2), { text: "€0.00", was: null });
  assert.deepEqual(shippingPrice(t, eur(0), eur(0), 2), { text: "€0.00", was: null });
});

test("an applied code names its discount", () => {
  assert.deepEqual(codeMessage(t, { code: "WELCOME10", state: "applied" }, [welcome], 2), {
    text: "Applied: Welcome offer",
    tone: "success",
  });
});

test("a code short of its minimum says how much more", () => {
  assert.equal(
    message({ code: "WELCOME10", state: "minimum_not_met", needed: { subtotal: eur(1200) } }),
    "Add €12.00 more to use WELCOME10",
  );
  assert.equal(
    message({ code: "BULK", state: "minimum_not_met", needed: { quantity: 1 } }),
    "Add 1 more item to use BULK",
  );
  assert.equal(
    message({ code: "BULK", state: "minimum_not_met", needed: { quantity: 3 } }),
    "Add 3 more items to use BULK",
  );
});

test("a code lost to a better combination names the winner", () => {
  assert.equal(
    message({ code: "SHIP", state: "blocked_by_other", discount: "disc_2" }, [freeShipping]),
    "Can't be combined with Free shipping over €50",
  );
  assert.equal(
    message({ code: "SHIP", state: "blocked_by_other", discount: "disc_9" }),
    "Can't be combined with another discount in your cart",
  );
});

test("each other state has its message", () => {
  assert.equal(
    message({ code: "VIP", state: "not_eligible", condition: "signed_in" }),
    "Sign in to use VIP",
  );
  assert.equal(
    message({ code: "VIP", state: "not_eligible", condition: "customer_groups" }),
    "VIP isn't available for your account",
  );
  assert.equal(
    message({ code: "VIP", state: "not_eligible", condition: "markets" }),
    "VIP can't be used in this country",
  );
  assert.equal(
    message({ code: "KIT", state: "no_matching_items" }),
    "Nothing in your cart qualifies for KIT",
  );
  assert.equal(message({ code: "ONCE", state: "limit_reached" }), "ONCE can't be used any more");
  assert.equal(message({ code: "FIVE", state: "no_saving" }), "FIVE wouldn't lower your total");
  assert.equal(
    message({ code: "EU", state: "not_available_in_market" }),
    "EU can't be used in this country",
  );
  assert.equal(message({ code: "OLD", state: "expired" }), "OLD has expired");
  assert.equal(message({ code: "GONE", state: "invalid" }), "GONE is no longer valid");
  assert.equal(
    message({ code: "NEW", state: "unknown" }),
    "NEW doesn't apply to this cart right now",
  );
});

test("only an applied code is good news", () => {
  assert.equal(codeMessage(t, { code: "ONCE", state: "limit_reached" }, [], 2).tone, "warning");
});

test("promotion badges list each title once, at most two", () => {
  const promos = readPromotions({
    promotions: [
      { discount: "disc_1", title: "Buy 2, get 1 free", summary: "" },
      { discount: "disc_2", title: "Buy 2, get 1 free", summary: "" },
      { discount: "disc_3", title: "Volume discount", summary: "" },
      { discount: "disc_4", title: "Third", summary: "" },
    ],
  });
  assert.deepEqual(promotionBadges(promos), ["Buy 2, get 1 free", "Volume discount"]);
  assert.deepEqual(readPromotions({}), []);
});

test("an English page shows a discount's summary as the API wrote it", () => {
  assert.equal(discountSentence(t, welcome, 2), "10% off everything");
  assert.equal(discountSentence(t, freeShipping, 2), "Free shipping on orders over €50");
  assert.equal(discountSentence(t, { ...welcome, summary: "" }, 2), null);
});

test("another language never shows the API's English summary", () => {
  // A reward on items isn't worded: its title says it.
  assert.equal(discountSentence(sr, welcome, 2), null);
  assert.equal(discountSentence(sr, freeShipping, 2), "Besplatna dostava");
  // German content reads the English catalog: its words would be English too.
  assert.equal(discountSentence(de, welcome, 2), null);
  assert.equal(discountSentence(de, freeShipping, 2), null);
});

/** A sentence with its no-break spaces (Serbian money: "5,00 €") written as spaces. */
const plain = (sentence: string | null) => sentence?.replace(/\s/g, " ") ?? null;

/** A Serbian sentence for a reward off the order. */
const order = (value: Application["value"]) =>
  plain(discountSentence(sr, { ...welcome, value, target: { type: "subtotal" } }, 2));

/** A Serbian sentence for a reward off shipping, capped at `max_amount`. */
const shipping = (value: Application["value"], max_amount?: Record<string, Money> | null) =>
  plain(discountSentence(sr, { ...welcome, value, target: { type: "shipping", max_amount } }, 2));

test("off the order or shipping is worded in the page's language", () => {
  assert.equal(order({ type: "percent", basis_points: 1250 }), "12,5% popusta na porudžbinu");
  assert.equal(
    order({
      type: "amount",
      amounts: { mkt_1: eur(500), mkt_2: { amount: 600, currency: "USD" } },
    }),
    "5,00 € popusta na porudžbinu",
  );
  // Two markets in the cart's currency with different amounts: it can't say which.
  assert.equal(order({ type: "amount", amounts: { mkt_1: eur(500), mkt_2: eur(700) } }), null);
  assert.equal(order({ type: "fixed_price", amounts: { mkt_1: eur(500) } }), null);
  assert.equal(
    shipping({ type: "percent", basis_points: 10_000 }, { mkt_1: eur(1000) }),
    "Besplatna dostava do 10,00 €",
  );
  assert.equal(shipping({ type: "percent", basis_points: 5000 }), "50% popusta na dostavu");
  assert.equal(
    shipping({ type: "percent", basis_points: 5000 }, { mkt_1: eur(1000) }),
    "50% popusta na dostavu, najviše 10,00 €",
  );
  assert.equal(
    shipping({ type: "amount", amounts: { mkt_1: eur(300) } }),
    "3,00 € popusta na dostavu",
  );
  // A cap in another currency only: the sentence would leave it out.
  assert.equal(
    shipping(
      { type: "percent", basis_points: 10_000 },
      { mkt_2: { amount: 900, currency: "USD" } },
    ),
    null,
  );
});

test("a product's promotion summaries show on an English page only", () => {
  const promos = [
    { title: "Buy 2, get 1 free", summary: "Buy 2 of Grow kits, get 1 free" },
    { title: "Volume", summary: "" },
  ];
  assert.deepEqual(summarisedPromotions(t, promos), [promos[0]]);
  assert.deepEqual(summarisedPromotions(de, promos), []);
  assert.deepEqual(summarisedPromotions(sr, promos), []);
});

test("a typed code is trimmed; a blank one isn't sent", () => {
  assert.equal(typedCode("  welcome10 "), "welcome10");
  assert.equal(typedCode("   "), null);
});
