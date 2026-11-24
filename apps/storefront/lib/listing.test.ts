import assert from "node:assert/strict";
import { test } from "node:test";

import {
  ON_SALE,
  listingHref,
  nextPage,
  onSaleOnly,
  previousPage,
  readListing,
  searchKey,
  sortFacetValues,
  toMinor,
  withOnSale,
} from "./listing.ts";

test("prices typed in major units become minor units", () => {
  assert.equal(toMinor("10.5", 2), 1050);
  assert.equal(toMinor("10", 2), 1000);
  assert.equal(toMinor("1050", 0), 1050);
  assert.equal(toMinor("10.555", 2), 1055);
  assert.equal(toMinor(undefined, 2), undefined);
  assert.equal(toMinor("99999999999999999", 2), undefined);
});

test("a listing's URL drops what isn't valid", () => {
  const state = readListing({ sort: "cheapest", min: "-5", max: "24.5", q: "  " });
  assert.equal(state.sort, undefined);
  assert.equal(state.min, undefined);
  assert.equal(state.max, "24.5");
  assert.equal(state.q, undefined);
});

test("pages go forward and back to the first", () => {
  const first = readListing({});
  const second = nextPage(first, "c2");
  const third = nextPage(second, "c3");
  assert.deepEqual(previousPage(third), { ...second, back: ["-"] });
  const back = previousPage(second);
  assert.equal(back?.cursor, undefined);
  assert.equal(previousPage(first), undefined);
});

test("Previous goes back at most 30 pages", () => {
  let state = readListing({});
  for (let page = 1; page <= 40; page++) state = nextPage(state, `c${page}`);
  assert.equal(state.back.length, 30);
  assert.equal(
    readListing({ back: Array.from({ length: 40 }, (_, i) => `c${i}`) }).back.length,
    30,
  );
});

test("the first page's URL carries no history", () => {
  assert.equal(listingHref("/c/shirts", { filters: [], back: ["c1"] }), "/c/shirts");
  assert.equal(
    listingHref("/c/shirts", { filters: ["option.color:red"], cursor: "c2", back: ["-"] }),
    "/c/shirts?filter=option.color%3Ared&cursor=c2&back=-",
  );
});

test("the on-sale toggle adds its filter and starts again from the first page", () => {
  const state = { ...readListing({ filter: "vendor:Acme" }), cursor: "c2", back: ["-"] };
  const on = withOnSale(state, true);
  assert.deepEqual(on.filters, ["vendor:Acme", ON_SALE]);
  assert.equal(on.cursor, undefined);
  assert.equal(onSaleOnly(on), true);
  assert.equal(listingHref("/hr/sale", on), "/hr/sale?filter=vendor%3AAcme&filter=on_sale%3Atrue");
});

test("turning the on-sale toggle off drops every on_sale filter", () => {
  const state = readListing({ filter: ["on_sale:true", "on_sale:false", "tag:new"] });
  assert.deepEqual(withOnSale(state, false).filters, ["tag:new"]);
  assert.equal(onSaleOnly(withOnSale(state, false)), false);
});

test("a search in another language is another cache entry", () => {
  const query = { country: "DE", sort: "newest", limit: 8 };
  const de = searchKey({ ...query, language: "de" });
  assert.notDeepEqual(de, searchKey({ ...query, language: "en" }));
  assert.deepEqual(de.slice(0, 3), ["search", "DE", "de"]);
  assert.notDeepEqual(de, searchKey(query));
});

test("a search in another country is another cache entry", () => {
  const query = { language: "de", sort: "newest" };
  assert.notDeepEqual(
    searchKey({ ...query, country: "DE" }),
    searchKey({ ...query, country: "AT" }),
  );
});

test("one search written in another order is one cache entry", () => {
  assert.deepEqual(
    searchKey({ country: "DE", language: "de", filter: ["tag:a", "tag:b"], limit: 4 }),
    searchKey({ limit: 4, filter: ["tag:a", "tag:b"], language: "de", country: "DE" }),
  );
  // An absent parameter is no parameter.
  assert.deepEqual(
    searchKey({ country: "DE", language: "de", cursor: undefined }),
    searchKey({ country: "DE", language: "de" }),
  );
});

test("facet values keep the most results first, ties by what they show in the page's language", () => {
  const values = [
    { value: "blue", label: "Blau", count: 2 },
    { value: "green", label: "Grün", count: 5 },
    { value: "oyster", label: "Austernpilz", count: 2 },
    { value: "red", label: "Rot", count: 2 },
  ];
  assert.deepEqual(
    sortFacetValues(values, "de-DE").map((v) => v.value),
    ["green", "oyster", "blue", "red"],
  );
  // The API's answer is left as it came.
  assert.deepEqual(
    values.map((v) => v.value),
    ["blue", "green", "oyster", "red"],
  );
});

test("facet values without a label sort by their value, numbers by size, letters by language", () => {
  const values = [
    { value: "10 ml", count: 1 },
    { value: "5 ml", count: 1 },
    { value: "Zebra", label: null, count: 1 },
    { value: "Äpfel", count: 1 },
  ];
  assert.deepEqual(
    sortFacetValues(values, "de-DE").map((v) => v.value),
    ["5 ml", "10 ml", "Äpfel", "Zebra"],
  );
  // Swedish sorts Ä after Z.
  assert.deepEqual(
    sortFacetValues(values, "sv-SE").map((v) => v.value),
    ["5 ml", "10 ml", "Zebra", "Äpfel"],
  );
});
