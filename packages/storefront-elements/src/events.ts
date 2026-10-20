// The events an app's storefront element may send (design note 0029, §7;
// docs/apps/elements.md). Each is a `CustomEvent` that bubbles and crosses shadow roots,
// its data in `detail`. Elements talk to the storefront only through these and the app's
// proxy.

export interface ElementEvents {
  /** The element can be submitted (a payment form loaded): a storefront paying with its own button (`submit_mode: "external"`) enables it now. */
  "hypha:ready": Record<string, never>;
  /** The shopper finished paying in the element (the payment's final state comes from the app reporting to Hypha). */
  "hypha:payment-confirmed": { paymentId: string };
  /** Something the shopper should see; the storefront shows `message`. */
  "hypha:error": { message: string };
  /** The element's content height changed, in CSS pixels. */
  "hypha:resize": { height: number };
}

export type ElementEventType = keyof ElementEvents;

export const ELEMENT_EVENT_TYPES: readonly ElementEventType[] = [
  "hypha:ready",
  "hypha:payment-confirmed",
  "hypha:error",
  "hypha:resize",
];

/** Listens for one event type on `element`; returns the function that stops listening. */
export function on<T extends ElementEventType>(
  element: EventTarget,
  type: T,
  handler: (detail: ElementEvents[T], event: CustomEvent<ElementEvents[T]>) => void,
): () => void {
  const listener = (event: Event) => {
    if (isCustomEvent<ElementEvents[T]>(event)) handler(event.detail, event);
  };
  element.addEventListener(type, listener);
  return () => element.removeEventListener(type, listener);
}

/**
 * A `CustomEvent`, by shape rather than `instanceof` (an element from another realm, an
 * iframe or a test DOM, has another `CustomEvent`). What `detail` holds is the element's
 * contract (docs/apps/elements.md), not something checked here.
 */
function isCustomEvent<D>(event: Event): event is CustomEvent<D> {
  return "detail" in event;
}

/** For element authors: sends one of the documented events from `element`. */
export function emit<T extends ElementEventType>(
  element: EventTarget,
  type: T,
  detail: ElementEvents[T],
): boolean {
  return element.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
}
