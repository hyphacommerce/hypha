import assert from "node:assert/strict";
import { test } from "node:test";

import { BridgeError, createBridge } from "./bridge.ts";
import { fakeFrame, flush } from "./testing.ts";
import { type Envelope, checkOrigin, isEnvelope } from "./protocol.ts";

const ADMIN = "https://admin.shop.example";
const APP = "https://app.example";

/** A host that answers every request with `answer(message)`. */
function answering(answer: (message: Envelope) => unknown) {
  const frame = fakeFrame(ADMIN, APP);
  frame.admin.addEventListener("message", (event) => {
    const message = event.data as Envelope;
    if (message.type === "ready" || message.type === "resize") return;
    frame.iframe.contentWindow.postMessage(
      { hypha: 1, id: message.id, type: "response", payload: answer(message) },
      APP,
    );
  });
  return frame;
}

test("refuses * and anything that isn't an exact origin", () => {
  for (const origin of ["*", "null", "https://admin.example/path", "admin.example", ""]) {
    assert.throws(() => checkOrigin(origin, "hostOrigin"), TypeError);
  }
  assert.equal(checkOrigin(ADMIN, "hostOrigin"), ADMIN);
});

test("announces itself, posting only to the host's origin", async () => {
  const frame = fakeFrame(ADMIN, APP);
  createBridge({ hostOrigin: ADMIN, window: frame.app });
  assert.equal(frame.toAdmin.length, 1);
  const [ready] = frame.toAdmin;
  assert.ok(ready);
  assert.equal(ready.targetOrigin, ADMIN);
  const message = ready.message;
  assert.ok(isEnvelope(message));
  assert.deepEqual([message.type, message.payload], ["ready", { version: 1 }]);
});

test("getSessionToken resolves with the host's token", async () => {
  const frame = answering(() => ({ ok: true, result: { token: "jwt" } }));
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app });
  assert.equal(await bridge.getSessionToken(), "jwt");
});

test("requests carry their payloads", async () => {
  const seen: Envelope[] = [];
  const frame = answering((message) => {
    seen.push(message);
    return message.type === "resource_picker"
      ? { ok: true, result: { selection: [{ id: "prod_1" }, { id: "prod_2" }] } }
      : { ok: true, result: null };
  });
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app });
  await bridge.navigate("/settings");
  await bridge.toast("Saved", { tone: "success" });
  await bridge.toast("Hello");
  assert.deepEqual(await bridge.openResourcePicker({ type: "product", multiple: true }), [
    "prod_1",
    "prod_2",
  ]);
  await bridge.close();
  assert.deepEqual(
    seen.map((m) => [m.type, m.payload]),
    [
      ["navigate", { path: "/settings" }],
      ["toast", { message: "Saved", tone: "success" }],
      ["toast", { message: "Hello", tone: "info" }],
      ["resource_picker", { type: "product", multiple: true }],
      ["close", {}],
    ],
  );
  // Every ID is distinct.
  assert.equal(new Set(seen.map((m) => m.id)).size, seen.length);
});

test("an error response rejects with its code", async () => {
  const frame = answering(() => ({ ok: false, error: { code: "cancelled", message: "none" } }));
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app });
  await assert.rejects(
    bridge.openResourcePicker({ type: "order" }),
    (error: unknown) => error instanceof BridgeError && error.code === "cancelled",
  );
});

test("a result that breaks the protocol is failed", async () => {
  const frame = answering((message) =>
    message.type === "session_token"
      ? { ok: true, result: { token: 42 } }
      : { ok: false, error: { code: "exploded", message: "?" } },
  );
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app });
  for (const call of [bridge.getSessionToken(), bridge.navigate("/x")]) {
    await assert.rejects(
      call,
      (error: unknown) => error instanceof BridgeError && error.code === "failed",
    );
  }
});

test("ignores answers from another origin or another window", async () => {
  const frame = fakeFrame(ADMIN, APP);
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app, timeoutMs: 50 });
  const token = bridge.getSessionToken();
  const request = frame.toAdmin.at(-1)?.message as Envelope;
  const response = {
    hypha: 1,
    id: request.id,
    type: "response",
    payload: { ok: true, result: { token: "evil" } },
  };
  frame.app.deliver(response, "https://evil.example", frame.parent);
  frame.app.deliver(response, ADMIN, { postMessage() {} });
  await assert.rejects(
    token,
    (error: unknown) => error instanceof BridgeError && error.code === "timeout",
  );
});

test("resize reports whole pixels and skips nonsense", () => {
  const frame = fakeFrame(ADMIN, APP);
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app });
  bridge.resize(120.2);
  bridge.resize(Number.NaN);
  bridge.resize(-1);
  const resizes = frame.toAdmin
    .map((p) => p.message as Envelope)
    .filter((m) => m.type === "resize");
  assert.deepEqual(
    resizes.map((m) => m.payload),
    [{ height: 121 }],
  );
});

test("autoResize reports through the watcher and stops on destroy", () => {
  const frame = fakeFrame(ADMIN, APP);
  let report: ((height: number) => void) | undefined;
  let stopped = false;
  const bridge = createBridge({
    hostOrigin: ADMIN,
    window: frame.app,
    autoResize: true,
    heightWatcher: (onHeight) => {
      report = onHeight;
      return () => {
        stopped = true;
      };
    },
  });
  report?.(300);
  const last = frame.toAdmin.at(-1)?.message as Envelope;
  assert.deepEqual([last.type, last.payload], ["resize", { height: 300 }]);
  bridge.destroy();
  assert.equal(stopped, true);
  assert.equal(frame.app.listenerCount, 0);
});

test("destroy rejects what's pending", async () => {
  const frame = fakeFrame(ADMIN, APP);
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app });
  const token = bridge.getSessionToken();
  bridge.destroy();
  await assert.rejects(
    token,
    (error: unknown) => error instanceof BridgeError && error.code === "cancelled",
  );
  await flush();
});
