// The first extension points' request and response bodies (design note 0029, §3.1),
// written by hand. The contract is hypha-extensions' DTOs, published as
// `openapi/extensions.json`; `pnpm --filter @hypha/app generate` turns that document
// into `generated/extensions.ts`, which then replaces these. Until the document exists,
// `generated/extensions.ts` re-exports this file.

/** Integer minor units and an ISO 4217 code, as everywhere in Hypha's API. */
export interface Money {
  amount: number;
  currency: string;
}

/** A postal address as Hypha sends it; personal fields are absent without their scopes (AP10). */
export interface PostalAddress {
  first_name?: string | null;
  last_name?: string;
  company?: string | null;
  line1?: string;
  line2?: string | null;
  city?: string;
  province?: string | null;
  postal_code?: string | null;
  country: string;
  phone?: string | null;
}

// ---------- shipping.rates v1 (today's callback, unchanged: AP9) ----------

export interface ShippingRatesRequest {
  /** `rreq_…`, also in `Hypha-Request-Id`. */
  id: string;
  /** RFC 3339, UTC. */
  sent_at: string;
  request: {
    currency: string;
    origin: PostalAddress | null;
    destination: PostalAddress;
    items: {
      variant: string;
      sku: string | null;
      quantity: number;
      unit_price: Money;
      weight_grams: number | null;
    }[];
    subtotal: Money;
  };
}

export interface ShippingRatesResponse {
  rates: {
    service: string;
    name: string;
    amount: Money;
    estimate?: { min_days: number; max_days: number } | null;
    reference?: string | null;
  }[];
}

// ---------- payments.* v1 ----------

export interface PaymentsStartRequest {
  /** `pay_…`: a good idempotency key for the provider. */
  payment: string;
  order: string;
  order_number: number;
  /** `pm_…`. */
  method: string;
  amount: Money;
  /** Where to send the shopper back to after a redirect, when the storefront gave one. */
  return_url?: string | null;
}

export type NextAction =
  | { type: "redirect"; url: string }
  /** A storefront element (§7) to mount with these props. */
  | { type: "client_widget"; element: string; props: Record<string, unknown> }
  | { type: "instructions"; text: string };

export interface PaymentsStartResponse {
  next_action: NextAction;
  /** The provider's reference (a PaymentIntent's ID). */
  reference?: string | null;
}

export interface PaymentActionRequest {
  payment: string;
  order: string;
  /** For a partial capture or refund; the whole payment when absent. */
  amount?: Money | null;
  /** The provider's reference from `payments.start`. */
  reference?: string | null;
}

export type PaymentActionResponse =
  | { status: "accepted"; reference?: string | null }
  | { status: "refused"; reason: string };

// ---------- search.query v1 ----------

export interface SearchQueryRequest {
  query: string;
  /** `mkt_…`: whose prices and stock. */
  market: string;
  /** `chan_…`. */
  channel?: string | null;
  currency: string;
  /** Facet names to count. */
  facets?: string[];
  /** Facet name to the values chosen. */
  filters?: Record<string, string[]>;
  limit: number;
  offset: number;
}

export interface SearchQueryResponse {
  /** In order. */
  hits: { product: string; variant?: string | null }[];
  /** Facet name to value to count. */
  facets: Record<string, Record<string, number>>;
  total: number;
}

/** Each point's newest version and bodies. */
export interface ExtensionPoints {
  "shipping.rates": { version: 1; request: ShippingRatesRequest; response: ShippingRatesResponse };
  "payments.start": { version: 1; request: PaymentsStartRequest; response: PaymentsStartResponse };
  "payments.capture": {
    version: 1;
    request: PaymentActionRequest;
    response: PaymentActionResponse;
  };
  "payments.void": { version: 1; request: PaymentActionRequest; response: PaymentActionResponse };
  "payments.refund": { version: 1; request: PaymentActionRequest; response: PaymentActionResponse };
  "search.query": { version: 1; request: SearchQueryRequest; response: SearchQueryResponse };
}
