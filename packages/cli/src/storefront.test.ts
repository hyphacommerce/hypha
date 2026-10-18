import assert from "node:assert/strict";
import { test } from "node:test";

import type { Deployment } from "./deployment.ts";
import { connectStorefront, linkOrders, orderUrl } from "./storefront.ts";

function deployment(target: Deployment["target"] = "vps"): Deployment {
  return {
    version: 1,
    target,
    remoteDir: "/opt/hypha",
    buildOn: "server",
    scheme: target === "local" ? "http" : "https",
    domains: { api: "api.example.com", media: "media.example.com", storefront: "shop.example.com" },
  };
}

type Call = { method: string; path: string; body?: unknown };

/** A staff session answering from `answers` by `METHOD path`, recording each call. */
function fakeStaff(answers: Record<string, unknown>) {
  const calls: Call[] = [];
  const staff = {
    async call<T>(method: string, path: string, body?: unknown): Promise<T> {
      calls.push(body === undefined ? { method, path } : { method, path, body });
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- the test names each route's answer
      return answers[`${method} ${path}`] as T;
    },
  };
  return { staff, calls };
}

const STORE = {
  "GET /admin/channels": { items: [{ id: "chan_1", kind: "online_store" }] },
  "POST /admin/apps": { secret: "hypha_as_hook" },
  "POST /admin/api-keys": { token: "hypha_pk_shop" },
};

test("the order link is the storefront's country-less order route, with both placeholders", () => {
  assert.equal(orderUrl(deployment()), "https://shop.example.com/order/{order}?token={token}");
  assert.equal(
    orderUrl(deployment("local")),
    "http://shop.example.com/order/{order}?token={token}",
  );
});

test("a store without order links gets the storefront's", async () => {
  const { staff, calls } = fakeStaff({
    "GET /admin/notification-settings": { order_url: null },
  });
  assert.equal(
    await linkOrders(deployment(), staff),
    "https://shop.example.com/order/{order}?token={token}",
  );
  assert.deepEqual(calls.at(-1), {
    method: "PATCH",
    path: "/admin/notification-settings",
    body: { order_url: "https://shop.example.com/order/{order}?token={token}" },
  });
});

test("a store's own order links are kept", async () => {
  const { staff, calls } = fakeStaff({
    "GET /admin/notification-settings": { order_url: "https://other.example/o/{order}" },
  });
  assert.equal(await linkOrders(deployment(), staff), null);
  assert.deepEqual(
    calls.map((c) => c.method),
    ["GET"],
  );
});

test("connecting the storefront links the orders before the app and the key", async () => {
  const { staff, calls } = fakeStaff({
    ...STORE,
    "GET /admin/notification-settings": { order_url: null },
  });
  assert.deepEqual(await connectStorefront(deployment(), staff), {
    key: "hypha_pk_shop",
    webhookSecret: "hypha_as_hook",
    orderUrl: "https://shop.example.com/order/{order}?token={token}",
  });
  assert.deepEqual(
    calls.map((c) => `${c.method} ${c.path}`),
    [
      "GET /admin/channels",
      "GET /admin/notification-settings",
      "PATCH /admin/notification-settings",
      "POST /admin/apps",
      "POST /admin/api-keys",
    ],
  );
});

test("on this machine there's no webhook app, and the store's links stay", async () => {
  const { staff, calls } = fakeStaff({
    ...STORE,
    "GET /admin/notification-settings": { order_url: "https://mine.example/{order}" },
  });
  assert.deepEqual(await connectStorefront(deployment("local"), staff), {
    key: "hypha_pk_shop",
    webhookSecret: null,
    orderUrl: null,
  });
  assert.ok(!calls.some((c) => c.path === "/admin/apps"));
  assert.ok(!calls.some((c) => c.method === "PATCH"));
});
