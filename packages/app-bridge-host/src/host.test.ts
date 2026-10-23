// Both sides together: @hyphacommerce/app-bridge in the fake iframe, this package in the admin.

import assert from "node:assert/strict";
import { test } from "node:test";

import { BridgeError, createBridge } from "@hyphacommerce/app-bridge";
import type { Envelope, ResponseEnvelope } from "@hyphacommerce/app-bridge/protocol";
import { fakeFrame, flush } from "@hyphacommerce/app-bridge/testing";

import { type AttachOptions, BridgeHostError, attachBridge } from "./host.ts";

const ADMIN = "https://admin.shop.example";
const APP = "https://app.example";

function setup(options: Omit<AttachOptions, "appOrigin" | "window">) {
  const frame = fakeFrame(ADMIN, APP);
  const detach = attachBridge(frame.iframe, { appOrigin: APP, window: frame.admin, ...options });
  const bridge = createBridge({ hostOrigin: ADMIN, window: frame.app, timeoutMs: 100 });
  return { frame, detach, bridge };
}

function rejectsWith(code: string) {
  return (error: unknown) => error instanceof BridgeError && error.code === code;
}

test("refuses * as the app's origin", () => {
  const frame = fakeFrame(ADMIN, APP);
  assert.throws(
    () => attachBridge(frame.iframe, { appOrigin: "*", window: frame.admin }),
    TypeError,
  );
});

test("answers every request end to end", async () => {
  const calls: unknown[] = [];
  let ready = 0;
  const { bridge, frame } = setup({
    getSessionToken: async () => "jwt-1",
    navigate: (path) => {
      calls.push(["navigate", path]);
    },
    toast: (message, { tone }) => {
      calls.push(["toast", message, tone]);
    },
    pickResource: async ({ type, multiple }) => {
      calls.push(["pick", type, multiple]);
      return ["cus_1"];
    },
    onClose: () => {
      calls.push(["close"]);
    },
    onReady: () => {
      ready += 1;
    },
  });
  assert.equal(await bridge.getSessionToken(), "jwt-1");
  await bridge.navigate("/settings");
  await bridge.toast("Saved", { tone: "success" });
  assert.deepEqual(await bridge.openResourcePicker({ type: "customer" }), ["cus_1"]);
  await bridge.close();
  assert.equal(ready, 1);
  assert.deepEqual(calls, [
    ["navigate", "/settings"],
    ["toast", "Saved", "success"],
    ["pick", "customer", false],
    ["close"],
  ]);
  // The host only ever posted to the app's origin.
  assert.ok(frame.toApp.every((p) => p.targetOrigin === APP));
});

test("resize reaches onResize", async () => {
  const heights: number[] = [];
  const { bridge } = setup({ onResize: (height) => heights.push(height) });
  bridge.resize(480);
  await flush();
  assert.deepEqual(heights, [480]);
});

test("a missing handler is unsupported", async () => {
  const { bridge } = setup({});
  await assert.rejects(bridge.getSessionToken(), rejectsWith("unsupported"));
  await assert.rejects(bridge.toast("x"), rejectsWith("unsupported"));
});

test("a cancelled picker is cancelled", async () => {
  const { bridge } = setup({ pickResource: async () => null });
  await assert.rejects(bridge.openResourcePicker({ type: "order" }), rejectsWith("cancelled"));
});

test("payloads that break the rules are invalid_request", async () => {
  const { bridge } = setup({ navigate: () => {}, toast: () => {}, pickResource: async () => [] });
  await assert.rejects(bridge.navigate("https://evil.example"), rejectsWith("invalid_request"));
  await assert.rejects(bridge.navigate("//evil.example"), rejectsWith("invalid_request"));
  await assert.rejects(bridge.toast(""), rejectsWith("invalid_request"));
  await assert.rejects(bridge.toast("x".repeat(501)), rejectsWith("invalid_request"));
  await assert.rejects(
    bridge.openResourcePicker({ type: "invoice" as "order" }),
    rejectsWith("invalid_request"),
  );
});

test("a handler's own error is failed, without its message; BridgeHostError keeps its code", async () => {
  const { bridge } = setup({
    getSessionToken: async () => {
      throw new Error("database password is hunter2");
    },
    navigate: () => {
      throw new BridgeHostError("cancelled", "the person stayed");
    },
  });
  await assert.rejects(bridge.getSessionToken(), (error: unknown) => {
    return (
      error instanceof BridgeError && error.code === "failed" && !error.message.includes("hunter2")
    );
  });
  await assert.rejects(bridge.navigate("/x"), rejectsWith("cancelled"));
});

test("an unknown type is answered unsupported", async () => {
  const frame = fakeFrame(ADMIN, APP);
  attachBridge(frame.iframe, { appOrigin: APP, window: frame.admin });
  frame.parent.postMessage({ hypha: 1, id: "x-1", type: "print", payload: {} }, ADMIN);
  await flush();
  const response = frame.toApp.at(-1)?.message as ResponseEnvelope;
  assert.equal(response.id, "x-1");
  assert.deepEqual(response.payload, {
    ok: false,
    error: { code: "unsupported", message: 'unknown message type "print"' },
  });
});

test("ignores other origins, other windows and non-bridge messages", async () => {
  let asked = 0;
  const frame = fakeFrame(ADMIN, APP);
  attachBridge(frame.iframe, {
    appOrigin: APP,
    window: frame.admin,
    getSessionToken: async () => {
      asked += 1;
      return "jwt";
    },
  });
  const request: Envelope = { hypha: 1, id: "a-1", type: "session_token", payload: {} };
  frame.admin.deliver(request, "https://evil.example", frame.iframe.contentWindow);
  frame.admin.deliver(request, APP, { postMessage() {} });
  frame.admin.deliver({ ...request, hypha: 2 }, APP, frame.iframe.contentWindow);
  frame.admin.deliver("hello", APP, frame.iframe.contentWindow);
  await flush();
  assert.equal(asked, 0);
  assert.equal(frame.toApp.length, 0);
});

test("detach stops answering", async () => {
  const { bridge, detach, frame } = setup({ getSessionToken: async () => "jwt" });
  detach();
  assert.equal(frame.admin.listenerCount, 0);
  await assert.rejects(bridge.getSessionToken(), rejectsWith("timeout"));
});
