// `<stripe-payment>`: the storefront element for `checkout.payment` (design note 0029,
// §7; docs/apps/elements.md). Props (set as properties): `client_secret`,
// `publishable_key`, `return_url`, `payment_id`, as `payments.start` answered them
// (camelCase aliases work too). It loads Stripe.js from js.stripe.com (the only way
// Stripe allows), mounts the Payment Element, confirms with `redirect: "if_required"`,
// and emits `hypha:payment-confirmed {paymentId}`, `hypha:error {message}` and
// `hypha:resize {height}`. Themed by `--hypha-*` custom properties.
//
// Bundled by scripts/build-assets.ts into src/generated/assets.ts, served at
// /elements/payment.js.

const STRIPE_JS = "https://js.stripe.com/v3";
const TAG = "stripe-payment";

// The little of Stripe.js used here.
interface StripeElement {
  mount(target: HTMLElement): void;
  destroy(): void;
  on(event: "ready" | "change", handler: (event: { complete?: boolean }) => void): void;
}
interface StripeElements {
  create(type: "payment", options?: Record<string, unknown>): StripeElement;
  submit(): Promise<{ error?: { message?: string } }>;
}
interface PaymentIntentResult {
  error?: { message?: string; type?: string };
  paymentIntent?: { id: string; status: string };
}
interface StripeInstance {
  elements(options: Record<string, unknown>): StripeElements;
  confirmPayment(options: Record<string, unknown>): Promise<PaymentIntentResult>;
  retrievePaymentIntent(clientSecret: string): Promise<PaymentIntentResult>;
}
type StripeFactory = (publishableKey: string) => StripeInstance;

declare global {
  interface Window {
    Stripe?: StripeFactory;
  }
}

let loading: Promise<StripeFactory> | undefined;

/** Loads Stripe.js once per page, from js.stripe.com. */
export function loadStripe(doc: Document = document): Promise<StripeFactory> {
  const win = doc.defaultView as Window | null;
  if (win?.Stripe) return Promise.resolve(win.Stripe);
  loading ??= new Promise<StripeFactory>((resolve, reject) => {
    const existing = doc.querySelector<HTMLScriptElement>(`script[src="${STRIPE_JS}"]`);
    const script = existing ?? doc.createElement("script");
    const done = () => {
      const factory = (doc.defaultView as Window | null)?.Stripe;
      if (factory) resolve(factory);
      else {
        loading = undefined;
        reject(new Error("Stripe.js didn't load"));
      }
    };
    script.addEventListener("load", done, { once: true });
    script.addEventListener(
      "error",
      () => {
        loading = undefined;
        reject(new Error("Stripe.js couldn't be loaded"));
      },
      { once: true },
    );
    if (!existing) {
      script.src = STRIPE_JS;
      script.async = true;
      doc.head.append(script);
    }
  });
  return loading;
}

const PROPS = ["client_secret", "publishable_key", "return_url", "payment_id"] as const;
type Prop = (typeof PROPS)[number];
const ALIASES: Record<string, Prop> = {
  clientSecret: "client_secret",
  publishableKey: "publishable_key",
  returnUrl: "return_url",
  paymentId: "payment_id",
};

const STYLE = `
  :host { display: block; font-family: var(--hypha-font-family, inherit);
          color: var(--hypha-color-text, inherit); }
  .wrap { display: grid; gap: 12px; }
  button { font: inherit; font-weight: 600; padding: 12px 16px; border: 0; cursor: pointer;
           border-radius: var(--hypha-radius, 6px);
           background: var(--hypha-color-accent, #635bff); color: var(--hypha-color-background, #fff); }
  button[disabled] { opacity: .6; cursor: default; }
  button:focus-visible { outline: 2px solid var(--hypha-color-accent, #635bff); outline-offset: 2px; }
  .error { color: var(--hypha-color-error, #d92d20); margin: 0; min-height: 1.2em; }
  .hidden { display: none; }
`;

// `HTMLElement` doesn't exist outside a browser (the module may be imported by tests
// that provide one first).
export class StripePayment extends HTMLElement {
  #props: Record<Prop, string | null> = {
    client_secret: null,
    publishable_key: null,
    return_url: null,
    payment_id: null,
  };
  #root: ShadowRoot;
  #mountPoint: HTMLDivElement | undefined;
  #button: HTMLButtonElement;
  #error: HTMLParagraphElement;
  #stripe: StripeInstance | undefined;
  #elements: StripeElements | undefined;
  #element: StripeElement | undefined;
  #mountedFor: string | null = null;
  #busy = false;
  #observer: ResizeObserver | undefined;

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = STYLE;
    const wrap = document.createElement("div");
    wrap.className = "wrap";
    wrap.setAttribute("part", "container");
    // Stripe's iframes mount in the light DOM, slotted in: Stripe.js expects its
    // container in the document, and its iframes keep their own styles anyway.
    const slot = document.createElement("slot");
    slot.name = "stripe";
    this.#button = document.createElement("button");
    this.#button.type = "button";
    this.#button.textContent = this.getAttribute("label") ?? "Pay now";
    this.#button.disabled = true;
    this.#button.setAttribute("part", "button");
    this.#button.addEventListener("click", () => void this.#pay());
    this.#error = document.createElement("p");
    this.#error.className = "error";
    this.#error.setAttribute("role", "alert");
    this.#error.setAttribute("part", "error");
    wrap.append(slot, this.#button, this.#error);
    this.#root.append(style, wrap);
    for (const prop of PROPS) this.#defineProp(prop, prop);
    for (const [alias, prop] of Object.entries(ALIASES)) this.#defineProp(alias, prop);
  }

  #defineProp(name: string, prop: Prop) {
    // Properties set before the class upgraded the element shadow these: take them over.
    const early: unknown = Object.hasOwn(this, name) ? Reflect.get(this, name) : undefined;
    if (early !== undefined) Reflect.deleteProperty(this, name);
    Object.defineProperty(this, name, {
      configurable: true,
      enumerable: true,
      get: () => this.#props[prop],
      set: (value: unknown) => {
        this.#props[prop] = typeof value === "string" && value ? value : null;
        if (this.isConnected) void this.#mount();
      },
    });
    if (early !== undefined) Reflect.set(this, name, early);
  }

  connectedCallback() {
    if (typeof ResizeObserver === "function" && !this.#observer) {
      this.#observer = new ResizeObserver(() =>
        this.#emit("hypha:resize", { height: Math.ceil(this.getBoundingClientRect().height) }),
      );
      this.#observer.observe(this);
    }
    void this.#mount();
  }

  disconnectedCallback() {
    this.#observer?.disconnect();
    this.#observer = undefined;
    this.#element?.destroy();
    this.#element = undefined;
    this.#mountedFor = null;
  }

  #emit(type: string, detail: Record<string, unknown>) {
    this.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
  }

  #fail(text: string) {
    this.#error.textContent = text;
    this.#emit("hypha:error", { message: text });
  }

  /** `--hypha-*` from the page, as Stripe's Appearance API variables. */
  #appearance(): Record<string, unknown> {
    const css = getComputedStyle(this);
    const read = (name: string) => css.getPropertyValue(name).trim() || undefined;
    const variables: Record<string, string> = {};
    const map: [string, string][] = [
      ["--hypha-color-accent", "colorPrimary"],
      ["--hypha-color-text", "colorText"],
      ["--hypha-color-background", "colorBackground"],
      ["--hypha-color-error", "colorDanger"],
      ["--hypha-font-family", "fontFamily"],
      ["--hypha-radius", "borderRadius"],
    ];
    for (const [property, variable] of map) {
      const value = read(property);
      if (value) variables[variable] = value;
    }
    return { theme: "stripe", variables };
  }

  async #mount() {
    const { client_secret: clientSecret, publishable_key: publishableKey } = this.#props;
    if (!clientSecret || !publishableKey || this.#mountedFor === clientSecret) return;
    this.#mountedFor = clientSecret;
    this.#error.textContent = "";
    this.#button.disabled = true;
    let factory: StripeFactory;
    try {
      factory = await loadStripe(this.ownerDocument);
    } catch {
      this.#mountedFor = null;
      return this.#fail(
        "The payment form couldn't be loaded. Check your connection and try again.",
      );
    }
    if (this.#mountedFor !== clientSecret) return;
    this.#stripe = factory(publishableKey);
    // Already paid (a reload after paying): say so instead of asking again.
    const existing = await this.#stripe.retrievePaymentIntent(clientSecret).catch(() => undefined);
    const state = existing?.paymentIntent?.status;
    if (state === "succeeded" || state === "processing" || state === "requires_capture")
      return this.#confirmed();
    this.#elements = this.#stripe.elements({ clientSecret, appearance: this.#appearance() });
    this.#element?.destroy();
    this.#element = this.#elements.create("payment", { layout: "tabs" });
    if (!this.#mountPoint) {
      this.#mountPoint = document.createElement("div");
      this.#mountPoint.slot = "stripe";
      this.append(this.#mountPoint);
    }
    this.#element.on("ready", () => {
      this.#button.disabled = false;
    });
    this.#element.mount(this.#mountPoint);
  }

  #confirmed() {
    this.#button.disabled = true;
    this.#emit("hypha:payment-confirmed", { paymentId: this.#props.payment_id ?? "" });
  }

  async #pay() {
    const stripe = this.#stripe;
    const elements = this.#elements;
    if (!stripe || !elements || this.#busy) return;
    this.#busy = true;
    this.#button.disabled = true;
    this.#error.textContent = "";
    // Whether the shopper may press again on this intent. A declined card fails the
    // payment in Hypha (its `failed` is final), so the storefront starts a new one.
    let retry = true;
    try {
      const submitted = await elements.submit();
      if (submitted.error)
        return this.#fail(submitted.error.message ?? "Check your payment details.");
      const result = await stripe.confirmPayment({
        elements,
        clientSecret: this.#props.client_secret,
        confirmParams: { return_url: this.#props.return_url ?? location.href },
        redirect: "if_required",
      });
      if (result.error) {
        retry = result.error.type !== "card_error";
        return this.#fail(result.error.message ?? "The payment didn't go through.");
      }
      const status = result.paymentIntent?.status;
      if (status === "succeeded" || status === "processing" || status === "requires_capture") {
        retry = false;
        return this.#confirmed();
      }
      this.#fail("The payment needs another step. Please try again.");
    } catch {
      this.#fail("The payment didn't go through. Please try again.");
    } finally {
      this.#busy = false;
      this.#button.disabled = !retry;
    }
  }
}

if (!customElements.get(TAG)) customElements.define(TAG, StripePayment);
