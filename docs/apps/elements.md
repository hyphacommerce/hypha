# Storefront elements

How an app puts UI into a storefront it doesn't control (design note 0029, §7). The
storefront is the merchant's own code, in any framework, so an app's storefront UI is a
**custom element** (a web component) in an ES module the app hosts. The storefront finds
it through the API, loads it, places it, and listens to its events.
`@hyphacommerce/storefront-elements` does the loading and wiring; this page is the contract on
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
props in the checkout's payment step, before the order exists (design note 0031): on
`hypha:payment-confirmed` it checks the cart out, which places the order once the app
says the money is committed.

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
  | `--hypha-color-text`, `--hypha-color-background` | text and surface colours (the background is what fields sit on) |
  | `--hypha-color-text-secondary` | labels and hints |
  | `--hypha-color-border` | field borders |
  | `--hypha-color-accent` | buttons, focus rings |
  | `--hypha-color-error` | error text |
  | `--hypha-color-scheme` | `light` or `dark`; unset, judge it from the background |
  | `--hypha-radius` | corner radius |

  Fall back to sensible defaults when they're unset (`var(--hypha-radius, 6px)`). Colours
  may be any CSS colour, `light-dark()` pairs and `oklch()` included: an element that
  hands them to something outside the page (a provider's iframe) resolves them first,
  e.g. through a probe element's computed `color`.
  Inherited properties (font, colour) reach into the shadow root by themselves.
- **Size**: lay out to the width the storefront gives; announce height changes that a
  storefront can't see (content inside your own iframes) with `hypha:resize`.
- **Trust**: the element runs with the storefront page's privileges (the merchant
  trusts it by installing the app). Never read the page's DOM or cookies outside the
  element, and keep secrets out of props a storefront might log: a payment's client
  secret is fine, an API key isn't.

### One element, served by an app and by a spore

The same provider may be offered twice: as an app (its own service, installed over
OAuth) and as a spore (compiled into the API). Both serve the element, and a storefront
can't tell which it got, so they must behave alike: same tag, props, events and theming.
Hypha's Stripe integrations keep one source by hand, `src/browser/payment-element.ts`
in hyphacommerce/app-stripe and `element/payment-element.ts` in
hyphacommerce/spore-stripe, differing only in the header comment: a fix lands in one
and is copied to the other in the same change, and each repository's CI holds its built
bundle to its source. There's no shared package on purpose: a Stripe-specific element
has no home in Hypha's packages, and a dependency between the two repositories for one
file would cost more than copying it.

## Events

Each is a `CustomEvent` dispatched on the element with `bubbles: true` and
`composed: true` (so it leaves a shadow root), its data in `detail`:

| Event | `detail` | Meaning |
| --- | --- | --- |
| `hypha:payment-confirmed` | `{ paymentId: string }` | The shopper finished paying. The storefront checks the cart out (`POST /storefront/carts/{id}/checkout`), which places the order once the app says the money is committed (its report, or `payments.status` when asked: `409 payment_pending` until then), so the storefront trusts Hypha's answer, not the event. |
| `hypha:error` | `{ message: string }` | Something the shopper should see, in their language (plain text). |
| `hypha:resize` | `{ height: number }` | The element's content height in CSS pixels changed. |
| `hypha:ready` | `{}` | The element can be submitted (its payment form loaded): a storefront paying with its own button enables it now. |

### Paying with the storefront's own button

A payment element may let the storefront's main button pay (design note 0031: the
checkout's one "Pay", as Shopify's): the storefront sets the prop `submit_mode:
"external"`, and the element then hides its own button (and its own error text: the
storefront shows `hypha:error`'s message by its button), sends `hypha:ready` once it
can be submitted, and pays when the storefront calls its `submit()` method. The outcome
arrives as the usual events: `hypha:payment-confirmed` (the storefront checks the cart
out), or `hypha:error` (the shopper fixes the card and presses the button again). A form
that can't load sends `hypha:error` too, so a storefront showing a placeholder until
`hypha:ready` isn't left waiting. An
element that doesn't support it ignores the prop and keeps its own button.

Later versions may add events; storefronts ignore ones they don't know.

## Using one (the storefront's side)

Without a framework:

```ts
import { loadElement, mountElement, on } from "@hyphacommerce/storefront-elements";

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
import { HyphaElement } from "@hyphacommerce/storefront-elements/react";

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
