import assert from "node:assert/strict";
import { test } from "node:test";

import { breadcrumbLd, countryAlternates, decimal, plainText, productLd } from "./seo.ts";

test("rich text becomes one line of plain text, entities decoded", () => {
  assert.equal(
    plainText("<p>Grow &amp; harvest</p>\n<p>in&nbsp;10&#39;s days</p>"),
    "Grow & harvest in 10's days",
  );
});

test("no text is no description", () => {
  assert.equal(plainText("<p> </p>"), undefined);
  assert.equal(plainText(null), undefined);
});

test("a long text is cut at a word, with an ellipsis, within the limit", () => {
  const cut = plainText("one two three four five six", 15);
  assert.equal(cut, "one two three…");
  assert.ok((cut ?? "").length <= 15);
});

test("prices are decimals in the currency's digits", () => {
  assert.equal(decimal({ amount: 1050, currency: "EUR" }, 2), "10.50");
  assert.equal(decimal({ amount: 5, currency: "EUR" }, 2), "0.05");
  assert.equal(decimal({ amount: 1200, currency: "JPY" }, 0), "1200");
  assert.equal(decimal({ amount: 1, currency: "KWD" }, 3), "0.001");
});

test("alternates name each country's copy and the first as the default", () => {
  assert.deepEqual(
    countryAlternates("en", ["AT", "HR"], (c) => `/${c.toLowerCase()}/p/x`),
    { "en-AT": "/at/p/x", "en-HR": "/hr/p/x", "x-default": "/at/p/x" },
  );
  assert.equal(
    countryAlternates("en", ["AT"], () => "/"),
    undefined,
  );
});

test("breadcrumbs are numbered from one", () => {
  const ld = breadcrumbLd([
    { name: "Home", url: "https://s/at" },
    { name: "Kit", url: "https://s/at/p/kit" },
  ]);
  assert.deepEqual(
    ld.itemListElement.map((i) => [i.position, i.name]),
    [
      [1, "Home"],
      [2, "Kit"],
    ],
  );
});

const variant = (price: number | null, available: boolean | null) => ({
  id: "var_1",
  title: "Large",
  price: price === null ? null : { amount: price, currency: "EUR" },
  available,
});

function product(variants: ReturnType<typeof variant>[]) {
  return {
    title: "Oyster kit",
    description: "<p>Fruits in ten days.</p>",
    vendor: "Mycelium Works",
    product_type: "Grow kit",
    media: [{ kind: "image", url: "https://m/kit.jpg" }],
    variants,
  };
}

test("a product has one offer per priced variant, with its stock", () => {
  const ld = productLd(
    product([variant(2500, true), variant(3000, false)]),
    "https://s/at/p/kit",
    2,
  );
  assert.ok(ld);
  assert.deepEqual(
    ld.offers.map((o) => [o.price, o.availability]),
    [
      ["25.00", "https://schema.org/InStock"],
      ["30.00", "https://schema.org/OutOfStock"],
    ],
  );
  assert.deepEqual(ld.brand, { "@type": "Brand", name: "Mycelium Works" });
  assert.equal(ld.description, "Fruits in ten days.");
});

test("a product not sold in the country has no structured data", () => {
  assert.equal(productLd(product([variant(null, null)]), "https://s/at/p/kit", 2), null);
});
