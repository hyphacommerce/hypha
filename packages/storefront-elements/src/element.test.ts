// The React wrapper on the server: the bare tag, which upgrades in the browser once the
// module loads. TSX, so jiti compiles it on load.

import assert from "node:assert/strict";
import { test } from "node:test";

import { createJiti } from "jiti";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" } });
const { HyphaElement } = await jiti.import<typeof import("./element.tsx")>("./element.tsx");

test("renders the custom element's tag with its class, and loads nothing on the server", () => {
  let loads = 0;
  const html = renderToStaticMarkup(
    createElement(HyphaElement, {
      tag: "stripe-payment",
      module: "https://app.example/elements/payment.js",
      props: { clientSecret: "pi_1_secret" },
      className: "payment",
      load: async () => {
        loads += 1;
        return Object as unknown as CustomElementConstructor;
      },
    }),
  );
  assert.equal(html, '<stripe-payment class="payment"></stripe-payment>');
  // Props are properties, set in the browser: none leaks into the markup.
  assert.ok(!html.includes("pi_1_secret"));
  assert.equal(loads, 0);
});
