import assert from "node:assert/strict";
import { test } from "node:test";

import { defineExtensionHandler } from "./extensions.ts";
import { MAX_MESSAGE_LENGTH, message } from "./validation.ts";

test("a message is an error by default", () => {
  assert.deepEqual(message("$.shipping_address.postal_code", "alcohol_restricted", "No."), {
    severity: "error",
    code: "alcohol_restricted",
    path: "$.shipping_address.postal_code",
    message: "No.",
  });
});

test("a warning says so", () => {
  assert.equal(message("$", "late_delivery", "Late.", "warning").severity, "warning");
});

test("text is trimmed", () => {
  assert.equal(message("$", "x", "  No.  ").message, "No.");
});

test("a code that isn't snake_case is refused", () => {
  assert.throws(() => message("$", "Alcohol", "No."), TypeError);
  assert.throws(() => message("$", "alcohol_", "No."), TypeError);
});

test("blank or overlong text is refused", () => {
  assert.throws(() => message("$", "x", "  "), TypeError);
  assert.throws(() => message("$", "x", "a".repeat(MAX_MESSAGE_LENGTH + 1)), TypeError);
  assert.equal(message("$", "x", "a".repeat(MAX_MESSAGE_LENGTH)).message.length, 500);
});

// The guide's example: no alcohol to postal codes starting with 35 or 38 (the Canary
// Islands).
const noAlcohol = defineExtensionHandler("checkout.validate", (body) => {
  const postal = body.cart.shipping_address?.postal_code ?? "";
  const alcohol = body.cart.lines.some((line) => line.sku?.startsWith("WINE-"));
  if (!alcohol || !/^3[58]/.test(postal)) return { messages: [] };
  return {
    messages: [
      message("$.shipping_address.postal_code", "alcohol_restricted", "We can't ship wine here."),
    ],
  };
});

function cart(postal: string, sku: string) {
  return {
    step: "checkout" as const,
    cart: {
      id: "cart_1",
      market: "mkt_1",
      channel: "chan_1",
      country: "ES",
      currency: "EUR",
      customer: null,
      email: null,
      shipping_address: {
        country: "ES",
        first_name: null,
        company: null,
        line2: null,
        province: null,
        postal_code: postal,
        phone: null,
      },
      shipping_address_check: null,
      lines: [
        {
          variant: "var_1",
          product: "prod_1",
          sku,
          quantity: 1,
          unit_price: null,
          weight_grams: null,
        },
      ],
      shipping_groups: [],
      subtotal: { amount: 0, currency: "EUR" },
      total: { amount: 0, currency: "EUR" },
    },
  };
}

const context = { idempotencyKey: "k", store: "store_1", request: new Request("https://x") };

test("a handler answers messages built with message()", async () => {
  const answer = await noAlcohol.handle(cart("35001", "WINE-1"), context as never);
  assert.equal(answer.messages[0]?.code, "alcohol_restricted");
  assert.deepEqual(
    (await noAlcohol.handle(cart("28001", "WINE-1"), context as never)).messages,
    [],
  );
});
