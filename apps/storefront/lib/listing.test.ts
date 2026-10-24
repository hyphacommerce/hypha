import assert from "node:assert/strict";
import { test } from "node:test";

import {
  ON_SALE,
  listingHref,
  nextPage,
  onSaleOnly,
  previousPage,
  readListing,
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
