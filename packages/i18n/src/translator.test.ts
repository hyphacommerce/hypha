import assert from "node:assert/strict";
import { test } from "node:test";

import { mergeCatalogs } from "./catalogs.ts";
import { type Messages, type Missing, createTranslator } from "./translator.ts";

const en = {
  "cart.title": "Your cart",
  "cart.items_count": "{count, plural, =0 {No items} one {# item} other {# items}}",
  "cart.greeting": "Hello, {name}!",
  "cart.total": "Total: {amount, number, ::currency/EUR}",
  "cart.placed": "Placed on {at, date, long}",
  "cart.gift": "{gender, select, female {her gift} male {his gift} other {their gift}}",
  "cart.terms": "Accept the <link>terms</link> and the <b>privacy <i>policy</i></b>.",
  "cart.only_english": "Only in English",
} as const;
/** The same catalog without its keys in the type: any string is a key. */
const untyped: Messages = en;

test("formats a plain message and a placeholder", () => {
  const t = createTranslator("en", en);
  assert.equal(t("cart.title"), "Your cart");
  assert.equal(t("cart.greeting", { name: "Ada" }), "Hello, Ada!");
});

test("English plurals: one and other, and an exact match first", () => {
  const t = createTranslator("en", en);
  assert.equal(t("cart.items_count", { count: 0 }), "No items");
  assert.equal(t("cart.items_count", { count: 1 }), "1 item");
  assert.equal(t("cart.items_count", { count: 2 }), "2 items");
  assert.equal(t("cart.items_count", { count: 1000 }), "1,000 items");
});

test("Serbian (Latin) plurals: one, few and other", () => {
  const t = createTranslator("sr-Latn", {
    count: "{count, plural, one {# artikal} few {# artikla} other {# artikala}}",
  });
  const cases: [number, string][] = [
    [0, "0 artikala"],
    [1, "1 artikal"],
    [2, "2 artikla"],
    [4, "4 artikla"],
    [5, "5 artikala"],
    [11, "11 artikala"],
    [21, "21 artikal"],
    [22, "22 artikla"],
    [25, "25 artikala"],
  ];
  for (const [count, expected] of cases) assert.equal(t("count", { count }), expected);
});

test("select picks the branch, else other", () => {
  const t = createTranslator("en", en);
  assert.equal(t("cart.gift", { gender: "female" }), "her gift");
  assert.equal(t("cart.gift", { gender: "unknown" }), "their gift");
});

test("numbers and dates are formatted for the language", () => {
  const at = new Date(Date.UTC(2026, 2, 5, 12));
  const english = createTranslator("en", en);
  const german = createTranslator("de", en);
  assert.equal(english("cart.total", { amount: 1234.5 }), "Total: €1,234.50");
  assert.equal(german("cart.total", { amount: 1234.5 }), "Total: 1.234,50 €");
  assert.equal(english("cart.placed", { at }), "Placed on March 5, 2026");
  assert.equal(german("cart.placed", { at }), "Placed on 5. März 2026");
});

test("the locale formats numbers, the language stays the language", () => {
  const t = createTranslator("de", en, { locale: "de-CH" });
  assert.equal(t.language, "de");
  assert.equal(t.locale, "de-CH");
  const swiss = new Intl.NumberFormat("de-CH").format(12345);
  assert.notEqual(swiss, new Intl.NumberFormat("de").format(12345));
  assert.equal(t("cart.items_count", { count: 12345 }), `${swiss} items`);
  assert.equal(createTranslator("de", en).locale, "de");
});

test("a missing key falls back to the fallback catalog, then to the key", () => {
  const missing: Missing[] = [];
  const de: Messages = { "cart.title": "Ihr Warenkorb" };
  const t = createTranslator("de", de, {
    fallback: untyped,
    onMissing: (m) => missing.push(m),
  });
  assert.equal(t("cart.title"), "Ihr Warenkorb");
  assert.equal(t("cart.only_english"), "Only in English");
  assert.equal(t("cart.nowhere"), "cart.nowhere");
  assert.deepEqual(missing, [
    { key: "cart.only_english", language: "de", shown: "fallback" },
    { key: "cart.nowhere", language: "de", shown: "key" },
  ]);
});

test("the language's own message wins over the fallback's", () => {
  const t = createTranslator("de", { "cart.title": "Ihr Warenkorb" }, { fallback: en });
  assert.equal(t("cart.title"), "Ihr Warenkorb");
});

test("a missing key is reported once, however often it's read", () => {
  let reports = 0;
  const t = createTranslator("de", {}, { fallback: untyped, onMissing: () => reports++ });
  t("cart.title");
  t("cart.title");
  t("cart.nowhere");
  t("cart.nowhere");
  assert.equal(reports, 2);
});

test("an inherited property isn't a message", () => {
  const t = createTranslator("en", untyped);
  assert.equal(t("toString"), "toString");
  assert.equal(t.has("toString"), false);
});

test("has: the catalog's keys and the fallback's", () => {
  const t = createTranslator("de", { a: "A" }, { fallback: { b: "B" } });
  assert.equal(t.has("a"), true);
  assert.equal(t.has("b"), true);
  assert.equal(t.has("c"), false);
});

test("a message is compiled once per translator", () => {
  let reads = 0;
  const messages = {
    get greeting() {
      reads += 1;
      return "Hello, {name}!";
    },
  };
  const t = createTranslator("en", messages);
  assert.equal(t("greeting", { name: "Ada" }), "Hello, Ada!");
  assert.equal(t("greeting", { name: "Grace" }), "Hello, Grace!");
  assert.equal(reads, 1);
});

test("a message that can't be formatted throws without onError", () => {
  const t = createTranslator("en", { ...en, broken: "{count, plural, one {# item}" });
  assert.throws(() => t("cart.greeting"));
  assert.throws(() => t("broken"));
});

test("with onError, a message that can't be formatted shows its key", () => {
  const errors: string[] = [];
  const t = createTranslator(
    "en",
    { ...en, broken: "{count, plural, one {# item}" },
    { onError: (_error, key) => errors.push(key) },
  );
  assert.equal(t("cart.greeting"), "cart.greeting");
  assert.equal(t("broken"), "broken");
  assert.deepEqual(t.rich("broken"), ["broken"]);
  assert.deepEqual(errors, ["cart.greeting", "broken"]);
});

test("rich: tags are functions, given their chunks and a key each", () => {
  const t = createTranslator("en", en);
  interface Node {
    tag: string;
    key: number;
    children: (string | Node)[];
  }
  const node =
    (tag: string) =>
    (children: (string | Node)[], key: number): Node => ({ tag, key, children });
  const parts = t.rich<Node>("cart.terms", { link: node("a"), b: node("b"), i: node("i") });
  assert.equal(parts.length, 5);
  const [before, link, between, bold, after] = parts;
  assert.equal(before, "Accept the ");
  assert.deepEqual(link, { tag: "a", key: 0, children: ["terms"] });
  assert.equal(between, " and the ");
  assert.equal(after, ".");
  // The inner tag is formatted first: every tag of the message still has its own key.
  assert.deepEqual(bold, {
    tag: "b",
    key: 2,
    children: ["privacy ", { tag: "i", key: 1, children: ["policy"] }],
  });
});

test("rich: a tag may answer text, and plain values still format", () => {
  const t = createTranslator("en", { hello: "Hello, <b>{name}</b>!" });
  assert.deepEqual(t.rich("hello", { name: "Ada", b: (chunks) => `*${chunks.join("")}*` }), [
    "Hello, *Ada*!",
  ]);
});

test("rich: a message without tags is one part", () => {
  const t = createTranslator("en", en);
  assert.deepEqual(t.rich("cart.title"), ["Your cart"]);
});

test("keys are typed from the catalogs", () => {
  const t = createTranslator("en", en);
  assert.equal(t("cart.title"), "Your cart");
  // @ts-expect-error -- not a key of the catalog
  assert.equal(t("cart.titel"), "cart.titel");

  const merged = mergeCatalogs({ cart: { title: "Your cart" }, checkout: { pay: "Pay" } });
  const fromNamespaces = createTranslator("en", merged);
  assert.equal(fromNamespaces("checkout.pay"), "Pay");
  // @ts-expect-error -- `pay` is checkout's
  assert.equal(fromNamespaces("cart.pay"), "cart.pay");

  // Another language's catalog loaded as plain messages: the keys are named.
  type Key = keyof typeof merged;
  const loaded: Record<string, string> = { "cart.title": "Ihr Warenkorb" };
  const german = createTranslator<Key>("de", loaded, { fallback: merged });
  assert.equal(german("cart.title"), "Ihr Warenkorb");
  assert.equal(german("checkout.pay"), "Pay");
  // @ts-expect-error -- not a key of the source catalog
  assert.equal(german("cart.nope"), "cart.nope");
});

test("mergeCatalogs prefixes each key with its namespace", () => {
  const merged = mergeCatalogs({
    cart: { title: "Your cart", items_count: "{count} items" },
    checkout: { title: "Checkout" },
  });
  assert.deepEqual(merged, {
    "cart.title": "Your cart",
    "cart.items_count": "{count} items",
    "checkout.title": "Checkout",
  });
});
