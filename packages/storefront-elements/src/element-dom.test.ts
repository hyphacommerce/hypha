// The React wrapper in a DOM (happy-dom): loads the module, sets props as properties
// after the element upgrades, and forwards the documented events.

import assert from "node:assert/strict";
import { test } from "node:test";

import { Window } from "happy-dom";

const window = new Window({ url: "https://shop.example/" });
const globals = globalThis as Record<string, unknown>;
for (const key of [
  "window",
  "document",
  "HTMLElement",
  "customElements",
  "Node",
  "Event",
  "CustomEvent",
  "MutationObserver",
])
  globals[key] = (window as unknown as Record<string, unknown>)[key];
globals.IS_REACT_ACT_ENVIRONMENT = true;

const { createJiti } = await import("jiti");
const { act, createElement } = await import("react");
const { createRoot } = await import("react-dom/client");
const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" } });
const { HyphaElement } = await jiti.import<typeof import("./element.tsx")>("./element.tsx");
const { emit } = await import("./events.ts");
const { createElementLoader } = await import("./loader.ts");

/** An app's element: keeps what the storefront set before it upgraded (the documented pattern). */
class TestPayment extends (window.HTMLElement as unknown as typeof HTMLElement) {
  #clientSecret = "";
  upgradedWith = "";
  connectedCallback() {
    if (Object.hasOwn(this, "clientSecret")) {
      const value = (this as unknown as { clientSecret: string }).clientSecret;
      delete (this as unknown as { clientSecret?: string }).clientSecret;
      this.clientSecret = value;
    }
  }
  get clientSecret() {
    return this.#clientSecret;
  }
  set clientSecret(value: string) {
    this.#clientSecret = value;
    this.upgradedWith = value;
  }
}

test("loads, upgrades with its props and forwards events", async () => {
  const { loadElement } = createElementLoader({
    registry: window.customElements as unknown as CustomElementRegistry,
    importModule: async () => {
      window.customElements.define("test-payment", TestPayment as never);
    },
  });
  const container = window.document.createElement("div");
  window.document.body.append(container);
  const events: unknown[] = [];
  const root = createRoot(container as unknown as Element);
  await act(async () => {
    root.render(
      createElement(HyphaElement, {
        tag: "test-payment",
        module: "https://app.example/payment.js",
        props: { clientSecret: "pi_1_secret" },
        onEvent: (event) => events.push(event),
        load: loadElement,
      }),
    );
  });
  await act(async () => {
    await window.customElements.whenDefined("test-payment");
  });
  const element = container.querySelector("test-payment") as unknown as TestPayment;
  assert.ok(element);
  assert.equal(element.clientSecret, "pi_1_secret");
  assert.equal(element.upgradedWith, "pi_1_secret");
  assert.equal(element.getAttribute("clientSecret"), null);

  emit(element, "hypha:payment-confirmed", { paymentId: "pay_1" });
  emit(element, "hypha:resize", { height: 320 });
  assert.deepEqual(events, [
    { type: "hypha:payment-confirmed", detail: { paymentId: "pay_1" } },
    { type: "hypha:resize", detail: { height: 320 } },
  ]);

  await act(async () => root.unmount());
  emit(element, "hypha:error", { message: "late" });
  assert.equal(events.length, 2);
});

test("a load failure arrives as hypha:error", async () => {
  const container = window.document.createElement("div");
  const events: unknown[] = [];
  const root = createRoot(container as unknown as Element);
  await act(async () => {
    root.render(
      createElement(HyphaElement, {
        tag: "broken-element",
        module: "https://app.example/broken.js",
        onEvent: (event) => events.push(event),
        load: async () => {
          throw new Error("couldn't load https://app.example/broken.js");
        },
      }),
    );
  });
  assert.deepEqual(events, [
    { type: "hypha:error", detail: { message: "couldn't load https://app.example/broken.js" } },
  ]);
  await act(async () => root.unmount());
});

test.after(async () => {
  await window.happyDOM.close();
});
