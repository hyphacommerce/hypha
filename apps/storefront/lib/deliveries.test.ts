import assert from "node:assert/strict";
import { test } from "node:test";

import { Deliveries } from "./deliveries.ts";

test("a delivery is acted on once while it's remembered", async () => {
  const deliveries = new Deliveries();
  assert.equal(await deliveries.once("dlv_1", async () => "done"), "done");
  assert.equal(await deliveries.once("dlv_1", async () => "again"), undefined);
  assert.equal(await deliveries.once("dlv_2", async () => "other"), "other");
});

test("a delivery whose handling failed is acted on when it comes again", async () => {
  const deliveries = new Deliveries();
  await assert.rejects(
    deliveries.once("dlv_1", async () => {
      throw new Error("API down");
    }),
  );
  assert.equal(await deliveries.once("dlv_1", async () => "done"), "done");
});

test("a delivery is acted on again once it's forgotten", async () => {
  let now = 0;
  const deliveries = new Deliveries(1_000, 5_000, () => now);
  await deliveries.once("dlv_1", async () => "done");
  now = 1_000;
  assert.equal(await deliveries.once("dlv_1", async () => "again"), "again");
});

test("a delivery without an ID is always acted on", async () => {
  const deliveries = new Deliveries();
  assert.equal(await deliveries.once(null, async () => 1), 1);
  assert.equal(await deliveries.once(null, async () => 2), 2);
});
