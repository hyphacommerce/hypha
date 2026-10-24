import assert from "node:assert/strict";
import { test } from "node:test";

import { rankRelated } from "./related.ts";

const hit = (product: string, type?: string, vendor?: string, available = true) => ({
  product,
  product_type: type,
  vendor,
  available,
});

const subject = { id: "self", product_type: "Kit", vendor: "Acme" };

test("a shared tag and type rank above the maker alone, newest fill the rest", () => {
  const ranked = rankRelated(
    subject,
    {
      tags: [hit("tagged-kit", "Kit")],
      type: [hit("kit", "Kit"), hit("tagged-kit", "Kit")],
      vendor: [hit("acme-tool", "Tool", "Acme")],
      newest: [hit("self", "Kit"), hit("new", "Book")],
    },
    4,
  );
  assert.deepEqual(
    ranked.map((h) => h.product),
    ["tagged-kit", "kit", "acme-tool", "new"],
  );
});

test("what can't be bought goes last, and the shelf stops at its limit", () => {
  const ranked = rankRelated(
    subject,
    {
      tags: [hit("sold-out", "Kit", "Acme", false)],
      type: [hit("kit", "Kit")],
      vendor: [],
      newest: [hit("a"), hit("b")],
    },
    3,
  );
  assert.deepEqual(
    ranked.map((h) => h.product),
    ["kit", "a", "b"],
  );
});
