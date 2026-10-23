import assert from "node:assert/strict";
import { test } from "node:test";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { signRequest } from "./signature.ts";
import { webhookHandler, type WebhookContext, type WebhookEnvelope } from "./webhooks.ts";

const SECRET = "hypha_as_test";

// The envelope as Hypha's webhooks.rs serializes it (its own unit test's sample).
const envelope: WebhookEnvelope = {
  id: "evt_00000000000000000000000000000001",
  type: "product.created",
  version: 1,
  subject: { type: "product", id: "prod_00000000000000000000000000000002", version: 1 },
  actor: { type: "user", id: "user_00000000000000000000000000000003" },
  occurred_at: "2027-01-15T08:00:00Z",
  data: { handle: "mug" },
};

// The hand-written envelope stays assignable to the admin API's `EventEnvelope` (the same
// type `GET /admin/events/{id}` returns), apart from `data`, which is open there.
type Shape<T> = Omit<T, "data">;
const sameShape: Shape<AdminSchemas["EventEnvelope"]> = envelope satisfies Shape<WebhookEnvelope>;
void sameShape;

async function deliver(
  handle: (request: Request) => Promise<Response>,
  body: unknown,
  {
    secret = SECRET,
    store = "store_1",
    delivery = "dlv_1",
    raw,
  }: Partial<Record<string, string>> = {},
): Promise<Response> {
  const text = raw ?? JSON.stringify(body);
  return handle(
    new Request("https://app.example/hooks", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "Hypha-Signature": await signRequest(secret, text),
        "Hypha-Event-Id": envelope.id,
        "Hypha-Event-Type": envelope.type,
        "Hypha-Delivery-Id": delivery,
        "Hypha-Store": store,
      },
      body: text,
    }),
  );
}

test("verifies, parses and hands over the envelope with its delivery and store", async () => {
  const seen: [WebhookEnvelope, WebhookContext][] = [];
  const handle = webhookHandler({
    secret: SECRET,
    onEvent: (event, context) => {
      seen.push([event, context]);
    },
  });
  const response = await deliver(handle, envelope);
  assert.equal(response.status, 204);
  assert.equal(seen.length, 1);
  assert.deepEqual(seen[0]?.[0], envelope);
  assert.equal(seen[0]?.[1].deliveryId, "dlv_1");
  assert.equal(seen[0]?.[1].store, "store_1");
});

test("the secret is looked up by store, with the body as received", async () => {
  const handle = webhookHandler({
    secret: ({ store, rawBody }) => {
      assert.equal(JSON.parse(new TextDecoder().decode(rawBody)).id, envelope.id);
      return store === "store_1" ? SECRET : undefined;
    },
    onEvent: () => {},
  });
  assert.equal((await deliver(handle, envelope)).status, 204);
  const other = await deliver(handle, envelope, { store: "store_2" });
  assert.equal(other.status, 401);
  assert.deepEqual(await other.json(), {
    code: "invalid_signature",
    error: "the signature doesn't verify",
  });
});

test("a wrong signature is 401, a body that isn't an envelope 400, a GET 405", async () => {
  const handle = webhookHandler({ secret: SECRET, onEvent: () => {} });
  assert.equal((await deliver(handle, envelope, { secret: "wrong" })).status, 401);
  const notJson = await deliver(handle, undefined, { raw: "{" });
  assert.equal(notJson.status, 400);
  const shapeless = await deliver(handle, { ...envelope, actor: { type: "robot", id: null } });
  assert.equal(shapeless.status, 400);
  assert.equal(((await shapeless.json()) as { code: string }).code, "invalid_request");
  assert.equal((await handle(new Request("https://app.example/hooks"))).status, 405);
});

test("a failing handler is 500, so Hypha retries", async () => {
  const errors: unknown[] = [];
  const handle = webhookHandler({
    secret: SECRET,
    onEvent: () => {
      throw new Error("db down");
    },
    onError: (error) => errors.push(error),
  });
  const response = await deliver(handle, envelope);
  assert.equal(response.status, 500);
  assert.equal(errors.length, 1);
});

test("a retried delivery is handled once with a dedupe store", async () => {
  const handled = new Set<string>();
  let calls = 0;
  const handle = webhookHandler({
    secret: SECRET,
    onEvent: () => {
      calls += 1;
    },
    dedupe: {
      seen: (id) => handled.has(id),
      remember: (id) => {
        handled.add(id);
      },
    },
  });
  for (const delivery of ["dlv_1", "dlv_1", "dlv_2"]) {
    assert.equal((await deliver(handle, envelope, { delivery })).status, 204);
  }
  assert.equal(calls, 2);
});

test("a delivery whose handler failed isn't remembered, so the retry is handled", async () => {
  const handled = new Set<string>();
  let calls = 0;
  const handle = webhookHandler({
    secret: SECRET,
    onEvent: () => {
      calls += 1;
      if (calls === 1) throw new Error("db down");
    },
    dedupe: {
      seen: (id) => handled.has(id),
      remember: (id) => {
        handled.add(id);
      },
    },
  });
  assert.equal((await deliver(handle, envelope, { delivery: "dlv_1" })).status, 500);
  assert.equal((await deliver(handle, envelope, { delivery: "dlv_1" })).status, 204);
  assert.equal(calls, 2);
});
