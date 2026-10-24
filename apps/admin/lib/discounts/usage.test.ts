import assert from "node:assert/strict";
import { test } from "node:test";

import { customerLabel, orderLabel } from "./usage.ts";

test("an order shows by its number, else by its ID", () => {
  assert.equal(orderLabel({ order: "order_1", order_number: 1001 }), "#1001");
  assert.equal(orderLabel({ order: "order_1", order_number: null }), "order_1");
  assert.equal(orderLabel({ order: "order_1" }), "order_1");
});

test("a customer shows by name, else email, else ID", () => {
  const customer = "cus_1";
  assert.equal(
    customerLabel({ customer, first_name: "Ada", last_name: "Lovelace", email: "ada@x.test" }),
    "Ada Lovelace",
  );
  assert.equal(customerLabel({ customer, last_name: "Lovelace" }), "Lovelace");
  assert.equal(customerLabel({ customer, email: "ada@x.test" }), "ada@x.test");
  // Redacted for the caller: neither field is there.
  assert.equal(customerLabel({ customer }), "cus_1");
});
