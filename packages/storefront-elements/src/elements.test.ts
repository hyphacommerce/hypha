import assert from "node:assert/strict";
import { test } from "node:test";

import { ELEMENT_EVENT_TYPES, emit, on } from "./events.ts";
import { ElementLoadError, createElementLoader, mountElement, setProps } from "./loader.ts";

/** A `customElements` stand-in: `define` resolves `whenDefined`, like the real one. */
function fakeRegistry() {
  const defined = new Map<string, CustomElementConstructor>();
  const waiting = new Map<string, (c: CustomElementConstructor) => void>();
  const promises = new Map<string, Promise<CustomElementConstructor>>();
  return {
    get: (tag: string) => defined.get(tag),
    whenDefined(tag: string) {
      const known = defined.get(tag);
      if (known) return Promise.resolve(known);
      let promise = promises.get(tag);
      if (!promise) {
        promise = new Promise((resolve) => waiting.set(tag, resolve));
        promises.set(tag, promise);
      }
      return promise;
    },
    define(tag: string, constructor: CustomElementConstructor) {
      defined.set(tag, constructor);
      waiting.get(tag)?.(constructor);
    },
  };
}

const MODULE = "https://app.example/elements/payment.js";
class PaymentElement {
  readonly kind = "payment";
}
const Payment = PaymentElement as unknown as CustomElementConstructor;

test("imports a module once for concurrent and later loads", async () => {
  const registry = fakeRegistry();
  const imported: string[] = [];
  const { loadElement } = createElementLoader({
    registry,
    importModule: async (url) => {
      imported.push(url);
      registry.define("stripe-payment", Payment);
    },
  });
  const loaded = await Promise.all([
    loadElement({ module: MODULE, tag: "stripe-payment" }),
    loadElement({ module: MODULE, tag: "stripe-payment" }),
  ]);
  assert.deepEqual(loaded, [Payment, Payment]);
  assert.equal(await loadElement({ module: MODULE, tag: "stripe-payment" }), Payment);
  assert.deepEqual(imported, [MODULE]);
});

test("waits for a module that defines its tag later", async () => {
  const registry = fakeRegistry();
  const { loadElement } = createElementLoader({
    registry,
    importModule: async () => {
      setTimeout(() => registry.define("late-element", Payment), 5);
    },
  });
  assert.equal(await loadElement({ module: MODULE, tag: "late-element" }), Payment);
});

test("a module that never defines its tag is not_defined", async () => {
  const { loadElement } = createElementLoader({
    registry: fakeRegistry(),
    importModule: async () => {},
    timeoutMs: 10,
  });
  await assert.rejects(
    loadElement({ module: MODULE, tag: "never-defined" }),
    (error: unknown) => error instanceof ElementLoadError && error.code === "not_defined",
  );
});

test("a failed import can be retried", async () => {
  const registry = fakeRegistry();
  let attempts = 0;
  const { loadElement } = createElementLoader({
    registry,
    importModule: async () => {
      attempts += 1;
      if (attempts === 1) throw new Error("offline");
      registry.define("retry-me", Payment);
    },
  });
  await assert.rejects(
    loadElement({ module: MODULE, tag: "retry-me" }),
    (error: unknown) => error instanceof ElementLoadError && error.code === "import_failed",
  );
  assert.equal(await loadElement({ module: MODULE, tag: "retry-me" }), Payment);
  assert.equal(attempts, 2);
});

test("refuses bad tags and modules that aren't https (localhost aside)", async () => {
  const registry = fakeRegistry();
  const { loadElement } = createElementLoader({
    registry,
    importModule: async () => registry.define("dev-element", Payment),
  });
  for (const tag of ["div", "Stripe-Payment", "1-x", ""]) {
    await assert.rejects(
      loadElement({ module: MODULE, tag }),
      (error: unknown) => error instanceof ElementLoadError && error.code === "invalid_tag",
    );
  }
  for (const module of [
    "/elements/payment.js",
    "http://app.example/x.js",
    "http://localhost.example/x.js",
    "data:text/javascript,1",
  ]) {
    await assert.rejects(
      loadElement({ module, tag: "dev-element" }),
      (error: unknown) => error instanceof ElementLoadError && error.code === "invalid_module",
    );
  }
  assert.equal(
    await loadElement({ module: "http://localhost:4000/x.js", tag: "dev-element" }),
    Payment,
  );
});

test("a *.localhost module loads over http (a deployment on this machine)", async () => {
  const registry = fakeRegistry();
  const { loadElement } = createElementLoader({
    registry,
    importModule: async () => registry.define("dev-element", Payment),
  });
  assert.equal(
    await loadElement({ module: "http://api.hypha.localhost/x.js", tag: "dev-element" }),
    Payment,
  );
});

/** The least of a DOM: elements with properties, attributes and children. */
function fakeDocument() {
  class FakeElement extends EventTarget {
    readonly children: FakeElement[] = [];
    readonly attributes = new Map<string, string>();
    readonly tagName: string;
    readonly ownerDocument: unknown;
    constructor(tag: string, owner: unknown) {
      super();
      this.tagName = tag.toUpperCase();
      this.ownerDocument = owner;
    }
    setAttribute(name: string, value: string) {
      this.attributes.set(name, value);
    }
    appendChild(child: FakeElement) {
      this.children.push(child);
      return child;
    }
  }
  const document = {
    createElement: (tag: string) => new FakeElement(tag, document),
  };
  return { document, root: new FakeElement("div", document) };
}

test("mountElement sets props as properties, not attributes", () => {
  const { root } = fakeDocument();
  const options = { appearance: { theme: "night" } };
  const element = mountElement(root as unknown as Element, {
    tag: "stripe-payment",
    props: { clientSecret: "pi_1_secret", options },
  }) as unknown as {
    tagName: string;
    clientSecret: string;
    options: unknown;
    attributes: Map<string, string>;
  };
  assert.equal(element.tagName, "STRIPE-PAYMENT");
  assert.equal(element.clientSecret, "pi_1_secret");
  assert.equal(element.options, options);
  assert.equal(element.attributes.size, 0);
  assert.equal((root as unknown as { children: unknown[] }).children[0], element);
});

test("setProps replaces values", () => {
  const { root } = fakeDocument();
  const element = mountElement(root as unknown as Element, { tag: "x-y", props: { a: 1 } });
  setProps(element, { a: 2, b: "c" });
  assert.deepEqual(
    [(element as unknown as { a: number }).a, (element as unknown as { b: string }).b],
    [2, "c"],
  );
});

test("on receives the typed detail of emitted events, and stops", () => {
  const element = new EventTarget();
  const seen: unknown[] = [];
  const off = on(element, "hypha:payment-confirmed", (detail, event) => {
    seen.push([detail.paymentId, event.bubbles, event.composed]);
  });
  emit(element, "hypha:payment-confirmed", { paymentId: "pay_1" });
  off();
  emit(element, "hypha:payment-confirmed", { paymentId: "pay_2" });
  assert.deepEqual(seen, [["pay_1", true, true]]);
});

test("the documented events", () => {
  assert.deepEqual(ELEMENT_EVENT_TYPES, [
    "hypha:ready",
    "hypha:payment-confirmed",
    "hypha:error",
    "hypha:resize",
  ]);
});
