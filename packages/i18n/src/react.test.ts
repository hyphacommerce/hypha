import assert from "node:assert/strict";
import { test } from "node:test";

import { type ReactNode, createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { I18nProvider, useLanguage, useT } from "./react.ts";

const en = {
  "cart.title": "Your cart",
  "cart.count": "{count, plural, one {# item} other {# items}}",
  "cart.terms": "Accept the <link>terms</link> and the <b>policy</b>.",
};
type Key = keyof typeof en;

function Cart({ count }: { count: number }): ReactNode {
  const t = useT<Key>();
  return createElement(
    "section",
    { lang: useLanguage() },
    createElement("h1", null, t("cart.title")),
    createElement("p", null, t("cart.count", { count })),
    createElement(
      "small",
      null,
      t.rich<ReactNode>("cart.terms", {
        link: (chunks, key) => createElement("a", { key, href: "/terms" }, chunks),
        b: (chunks, key) => createElement("b", { key }, chunks),
      }),
    ),
  );
}

test("components under the provider translate", () => {
  const html = renderToStaticMarkup(
    createElement(
      I18nProvider,
      { language: "en", messages: en },
      createElement(Cart, { count: 2 }),
    ),
  );
  assert.equal(
    html,
    '<section lang="en"><h1>Your cart</h1><p>2 items</p>' +
      '<small>Accept the <a href="/terms">terms</a> and the <b>policy</b>.</small></section>',
  );
});

test("a key the language lacks comes from the fallback", () => {
  const html = renderToStaticMarkup(
    createElement(
      I18nProvider,
      { language: "de", messages: { "cart.title": "Ihr Warenkorb" }, fallback: en },
      createElement(Cart, { count: 1 }),
    ),
  );
  assert.match(html, /<section lang="de"><h1>Ihr Warenkorb<\/h1><p>1 item<\/p>/);
});

test("rich parts are children React takes without a missing-key warning", () => {
  const errors: unknown[][] = [];
  const original = console.error;
  console.error = (...args: unknown[]) => errors.push(args);
  try {
    renderToStaticMarkup(
      createElement(
        I18nProvider,
        { language: "en", messages: en },
        createElement(Cart, { count: 2 }),
      ),
    );
  } finally {
    console.error = original;
  }
  assert.deepEqual(errors, []);
});

test("a message that can't be formatted shows its key and is logged", () => {
  const errors: unknown[][] = [];
  const original = console.error;
  console.error = (...args: unknown[]) => errors.push(args);
  try {
    const html = renderToStaticMarkup(
      createElement(
        I18nProvider,
        { language: "en", messages: { ...en, "cart.count": "{count, plural, one {# item}" } },
        createElement(Cart, { count: 2 }),
      ),
    );
    assert.match(html, /<p>cart\.count<\/p>/);
  } finally {
    console.error = original;
  }
  assert.equal(errors.length, 1);
  assert.equal(errors[0]?.[0], "i18n: cart.count (en)");
});

function Both(): ReactNode {
  const t = useT();
  return createElement("p", null, `${t("cart.title")} / ${t("checkout.pay")} / ${t("shared")}`);
}

/** `Both` under an English provider and, inside it, one of `language`. */
function nested(language: string): string {
  return renderToStaticMarkup(
    createElement(
      I18nProvider,
      { language: "en", messages: { "cart.title": "Your cart", shared: "outer" } },
      createElement(
        I18nProvider,
        { language, messages: { "checkout.pay": "Pay", shared: "inner" } },
        createElement(Both),
      ),
    ),
  );
}

test("a nested provider adds its messages to the outer one's", () => {
  assert.equal(nested("en"), "<p>Your cart / Pay / inner</p>");
  // Another language starts over: the outer messages aren't its own.
  assert.equal(nested("de"), "<p>cart.title / Pay / inner</p>");
});

test("the hooks need a provider", () => {
  assert.throws(
    () => renderToStaticMarkup(createElement(Cart, { count: 1 })),
    /need an <I18nProvider>/,
  );
});
