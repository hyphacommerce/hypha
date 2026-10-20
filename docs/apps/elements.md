# Storefront elements

How an app puts UI into a storefront it doesn't control (design note 0029, §7). The
storefront is the merchant's own code, in any framework, so an app's storefront UI is a
**custom element** (a web component) in an ES module the app hosts. The storefront finds
it through the API, loads it, places it, and listens to its events.
`@hypha/storefront-elements` does the loading and wiring; this page is the contract on
both sides, so either can be written without it.

## Discovery

The app declares its elements in its manifest:

```json
"storefront": {
  "elements": [{ "id": "payment-element", "target": "checkout.payment",
                 "module": "/elements/payment.js", "tag": "stripe-payment" }]
}
```

A storefront asks `GET /storefront/app-extensions?target=checkout.payment` (with its
publishable key) and gets `{ app, tag, module, props }` per installed app: `module` is
an absolute URL, and `props` the values to set. For payments, the props come from the
payment's next action instead: `payments.start` answered
`client_widget { element, props }`, and the storefront mounts that element with those
props in the payment step.

Targets: `checkout.payment`, `product.block`, `cart.block`, `order.confirmation` (and
whatever later versions add: a storefront shows what it supports).

## Writing an element (the app's side)

The module defines one custom element per tag and nothing else global:

```js
// https://stripe-app.example/elements/payment.js
class StripePayment extends HTMLElement {
  #clientSecret = "";
  #root = this.attachShadow({ mode: "open" });

  connectedCallback() {
    // A storefront may set props before this module loaded: take them over.
    for (const key of ["clientSecret", "returnUrl"]) {
      if (Object.hasOwn(this, key)) {
        const value = this[key];
        delete this[key];
        this[key] = value;
      }
    }
    this.#render();
  }

  get clientSecret() { return this.#clientSecret; }
  set clientSecret(value) { this.#clientSecret = value; this.#render(); }

  async #pay() {
    try {
      const payment = await confirmWithStripe(this.#clientSecret);
      this.dispatchEvent(new CustomEvent("hypha:payment-confirmed", {
        detail: { paymentId: payment.hyphaPaymentId }, bubbles: true, composed: true }));
    } catch (error) {
      this.dispatchEvent(new CustomEvent("hypha:error", {
        detail: { message: "Your card was declined." }, bubbles: true, composed: true }));
    }
  }

  #render() { /* into this.#root */ }
}

if (!customElements.get("stripe-payment")) customElements.define("stripe-payment", StripePayment);
```

The rules:

- **Tag**: lowercase with a hyphen, prefixed with the app's name (`stripe-payment`,
  `reviews-summary`) so two apps never collide. The module defines it once (guard with
  `customElements.get`), and only the tags the manifest lists.
- **Module**: an ES module served over HTTPS (`http://localhost` or a `*.localhost` name on this machine in development) with
  CORS allowing any origin (`Access-Control-Allow-Origin: *`; it's a public script) and a
  long-lived cache under a versioned URL. No side effects beyond `customElements.define`:
  no global CSS, no globals, no listeners on `window` or `document` until connected.
- **Props are properties, not attributes.** The storefront sets each key of `props` as
  a JavaScript property, so values can be objects. Read them through setters, and take
  over properties set before the class upgraded (the `connectedCallback` loop above):
  a storefront may create the element before the module has loaded. Attributes are only
  for plain strings an app chooses to also accept.
- **Events** are the only way the element talks to the storefront (below). Everything
  else goes to the app: its own backend, or Hypha's app proxy
  (`/storefront/apps/{app}/proxy/…`, called by the storefront's server) when the element
  needs store context.
- **Styling**: render into a shadow root so the storefront's CSS can't break the
  element and the element's can't leak. Expose theming through CSS custom properties
  the storefront may set on the element or an ancestor, prefixed `--hypha-` for the
  shared ones and the app's name for its own:

  | Custom property | Meaning |
  | --- | --- |
  | `--hypha-font-family` | the storefront's body font |
  | `--hypha-color-text`, `--hypha-color-background` | text and surface colours |
  | `--hypha-color-accent` | buttons, focus rings |
  | `--hypha-color-error` | error text |
  | `--hypha-radius` | corner radius |

  Fall back to sensible defaults when they're unset (`var(--hypha-radius, 6px)`).
  Inherited properties (font, colour) reach into the shadow root by themselves.
- **Size**: lay out to the width the storefront gives; announce height changes that a
  storefront can't see (content inside your own iframes) with `hypha:resize`.
- **Trust**: the element runs with the storefront page's privileges (the merchant
  trusts it by installing the app). Never read the page's DOM or cookies outside the
  element, and keep secrets out of props a storefront might log: a payment's client
  secret is fine, an API key isn't.

## Events

Each is a `CustomEvent` dispatched on the element with `bubbles: true` and
`composed: true` (so it leaves a shadow root), its data in `detail`:

| Event | `detail` | Meaning |
| --- | --- | --- |
| `hypha:payment-confirmed` | `{ paymentId: string }` | The shopper finished paying. The storefront shows the confirmation; the payment's final state arrives by the app reporting to Hypha (`POST /admin/payments/{id}/events`), so the storefront reads the order rather than trusting the event. |
| `hypha:error` | `{ message: string }` | Something the shopper should see, in their language (plain text). |
| `hypha:resize` | `{ height: number }` | The element's content height in CSS pixels changed. |

Later versions may add events; storefronts ignore ones they don't know.

## Using one (the storefront's side)

Without a framework:

```ts
import { loadElement, mountElement, on } from "@hypha/storefront-elements";

await loadElement({ module, tag });                       // imports once, waits for the tag
const element = mountElement(container, { tag, props });  // props set as properties
const off = on(element, "hypha:payment-confirmed", ({ paymentId }) => showConfirmation(paymentId));
```

`loadElement` imports each module URL once (later and concurrent calls share it; a
failed import can be retried), refuses tags that aren't custom element names and
modules that aren't HTTPS, and fails with `not_defined` when the module doesn't define
the tag within ten seconds. Its errors are `ElementLoadError` with a `code`
(`invalid_tag`, `invalid_module`, `import_failed`, `not_defined`).

In React:

```tsx
import { HyphaElement } from "@hypha/storefront-elements/react";

<HyphaElement
  tag={action.element}
  module={moduleUrl}
  props={action.props}
  onEvent={(event) => {
    if (event.type === "hypha:payment-confirmed") router.push(confirmation(event.detail.paymentId));
    if (event.type === "hypha:error") setMessage(event.detail.message);
  }}
/>
```

It renders the bare tag on the server, loads the module in the browser, sets `props`
as properties (again after the upgrade), and reports a load failure as `hypha:error`.
Custom elements work the same in Vue, Svelte or plain HTML: set properties, listen for
the events.

**Content Security Policy:** list the installed apps' module origins in `script-src`
(discovery gives the URLs), so the storefront runs exactly the apps the merchant
installed.
