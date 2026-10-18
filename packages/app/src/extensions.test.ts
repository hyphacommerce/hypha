import assert from "node:assert/strict";
import { test } from "node:test";

import { ExtensionError, defineExtensionHandler, extensionHandler } from "./extensions.ts";
import { signRequest } from "./signature.ts";

const SECRET = "hypha_shs_test";

const start = defineExtensionHandler("payments.start", async (body, context) => ({
  next_action: { type: "client_widget", element: "stripe-payment", props: { secret: "cs_1" } },
  reference: `${body.payment}:${context.idempotencyKey}:${context.store}`,
}));

const capture = defineExtensionHandler("payments.capture", (body) => {
  if (body.amount?.amount === 0) throw new ExtensionError("zero_amount", "nothing to capture");
  if (body.amount?.amount === 13) throw new Error("boom");
  return { status: "accepted" };
});

const handle = extensionHandler({ secret: SECRET, handlers: [start, capture] });

async function call(
  point: string,
  body: unknown,
  { secret = SECRET, version = "1", raw }: { secret?: string; version?: string; raw?: string } = {},
): Promise<Response> {
  const text = raw ?? JSON.stringify(body);
  return handle(
    new Request("https://app.example/hypha/ext", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "Hypha-Signature": await signRequest(secret, text),
        "Hypha-Extension": point,
        "Hypha-Extension-Version": version,
        "Idempotency-Key": "idem_1",
        "Hypha-Store": "store_1",
      },
      body: text,
    }),
  );
}

const startBody = {
  payment: "pay_1",
  order: "ord_1",
  order_number: 1001,
  method: "pm_1",
  amount: { amount: 1999, currency: "EUR" },
};

test("dispatches by Hypha-Extension and passes the context", async () => {
  const response = await call("payments.start", startBody);
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), {
    next_action: { type: "client_widget", element: "stripe-payment", props: { secret: "cs_1" } },
    reference: "pay_1:idem_1:store_1",
  });
});

test("a bad signature is 401 before anything else", async () => {
  const response = await call("payments.start", startBody, { secret: "wrong" });
  assert.equal(response.status, 401);
  assert.deepEqual(await response.json(), {
    code: "invalid_signature",
    error: "the signature doesn't verify",
  });
});

test("an unknown point is 404, an unsupported version 400", async () => {
  assert.equal((await call("search.query", {})).status, 404);
  const response = await call("payments.start", startBody, { version: "2" });
  assert.equal(response.status, 400);
  assert.equal(((await response.json()) as { code: string }).code, "unsupported_version");
});

test("a body that isn't JSON is 400 invalid_request", async () => {
  const response = await call("payments.start", undefined, { raw: "{" });
  assert.equal(response.status, 400);
  assert.equal(((await response.json()) as { code: string }).code, "invalid_request");
});

test("an ExtensionError answers its code; anything else is 500 without details", async () => {
  const refused = await call("payments.capture", {
    payment: "p",
    order: "o",
    amount: { amount: 0, currency: "EUR" },
  });
  assert.equal(refused.status, 422);
  assert.deepEqual(await refused.json(), { code: "zero_amount", error: "nothing to capture" });
  const failed = await call("payments.capture", {
    payment: "p",
    order: "o",
    amount: { amount: 13, currency: "EUR" },
  });
  assert.equal(failed.status, 500);
  assert.deepEqual(await failed.json(), { code: "handler_failed", error: "the handler failed" });
});

test("only POST", async () => {
  const response = await handle(new Request("https://app.example/hypha/ext"));
  assert.equal(response.status, 405);
});

test("the secret may be looked up per request", async () => {
  const perStore = extensionHandler({
    secret: (request) => (request.headers.get("Hypha-Store") === "store_1" ? SECRET : undefined),
    handlers: [start],
  });
  const text = JSON.stringify(startBody);
  const response = await perStore(
    new Request("https://app.example/x", {
      method: "POST",
      headers: {
        "Hypha-Signature": await signRequest(SECRET, text),
        "Hypha-Extension": "payments.start",
        "Hypha-Store": "store_2",
      },
      body: text,
    }),
  );
  assert.equal(response.status, 401);
});
