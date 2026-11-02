import assert from "node:assert/strict";
import { test } from "node:test";

import { type CartMessage, blockedBy, placeMessages, placeOf, statusOf } from "./messages.ts";

const message = (path: string, severity = "warning", text = "Check this"): CartMessage => ({
  severity,
  code: "house_number_missing",
  path,
  message: text,
  source: { type: "format" },
});

test("a field's path places the message under that field", () => {
  assert.deepEqual(placeOf("$.shipping_address.line1"), { at: "field", field: "line1" });
});

test("an index path places the message by its group or line", () => {
  assert.deepEqual(placeOf("$.shipping_groups[0]"), { at: "group", index: 0 });
  assert.deepEqual(placeOf("$.lines[12]"), { at: "line", index: 12 });
});

test("an unknown path isn't placed", () => {
  assert.equal(placeOf("$.shipping_address.street"), undefined);
  assert.equal(placeOf("$.lines[01]"), undefined);
  assert.equal(placeOf("$.payment_method"), undefined);
});

test("the cart's messages and unknown ones go to the top", () => {
  const placed = placeMessages([message("$"), message("$.somewhere_new")]);
  assert.equal(placed.top.length, 2);
});

test("messages are grouped by where they're shown", () => {
  const placed = placeMessages([
    message("$.email"),
    message("$.shipping_address"),
    message("$.shipping_address.postal_code"),
    message("$.shipping_groups[1]"),
    message("$.lines[0]"),
  ]);
  assert.equal(placed.email.length, 1);
  assert.equal(placed.address.length, 1);
  assert.equal(placed.fields.postal_code?.length, 1);
  assert.equal(placed.groups[1]?.length, 1);
  assert.equal(placed.lines[0]?.length, 1);
  assert.equal(placed.top.length, 0);
});

test("a field shows its first error before its warnings", () => {
  const status = statusOf([
    message("$.shipping_address.line1", "warning", "Add a house number"),
    message("$.shipping_address.line1", "error", "No PO boxes"),
  ]);
  assert.deepEqual(status, { type: "error", message: "No PO boxes" });
});

test("a severity this build doesn't know shows as a warning", () => {
  assert.deepEqual(statusOf([message("$.email", "notice", "Hi")]), {
    type: "warning",
    message: "Hi",
  });
});

test("a blocked checkout's messages come from its details", () => {
  const error = {
    code: "checkout_blocked",
    details: { messages: [message("$.shipping_groups[0]", "error"), { path: 1 }] },
  };
  assert.deepEqual(blockedBy(error), [message("$.shipping_groups[0]", "error")]);
});

test("another error blocks nothing", () => {
  assert.deepEqual(blockedBy({ code: "total_changed", details: { messages: [] } }), []);
});
