// Payments in the admin (design note 0027): the method presets staff start from, and
// which orders staff can still mark paid. Plain data, for server and client code alike.

/** A starting point for a new method: all `manual`, staff record the money. */
export type PaymentPreset = {
  value: "cash_on_delivery" | "bank_transfer" | "custom";
  label: string;
  description: string;
  /** The name it starts with; staff can change it. */
  name: string;
  /** What the instructions field suggests: the shopper sees the instructions after checkout. */
  instructionsPlaceholder: string;
};

export const PAYMENT_PRESETS: PaymentPreset[] = [
  {
    value: "cash_on_delivery",
    label: "Cash on delivery",
    description: "The shopper pays the courier; you mark the order paid.",
    name: "Cash on delivery",
    instructionsPlaceholder: "Pay the courier in cash when your order arrives.",
  },
  {
    value: "bank_transfer",
    label: "Bank transfer",
    description: "The shopper transfers the total; you mark the order paid when it lands.",
    name: "Bank transfer",
    instructionsPlaceholder:
      "Transfer the total to IBAN HR12 3456 7890 1234 5678 9 (Your Store Ltd), with your order number as the reference.",
  },
  {
    value: "custom",
    label: "Other manual method",
    description: "Anything else you collect yourself: a card terminal, an invoice.",
    name: "",
    instructionsPlaceholder: "What the shopper should do to pay.",
  },
];

/** What a provider key reads as. */
export function providerLabel(provider: string): string {
  if (provider === "manual") return "Manual";
  return provider.startsWith("spore_") ? provider.slice("spore_".length) : provider;
}

/**
 * Whether staff can mark the order paid: not cancelled, and nothing collected yet
 * (`pending`, or `authorized` by a provider). The API takes the newest open payment, or
 * records a manual one of the total when there's none.
 */
export function canMarkPaid(order: {
  financial_status: string;
  cancelled_at?: string | null;
}): boolean {
  return !order.cancelled_at && MARKABLE.includes(order.financial_status);
}

/** What `canMarkPaid` allows. A partly paid order isn't: marking it would record the
 * whole total again. */
const MARKABLE = ["pending", "authorized"];

/** Statuses with money still to collect: the orders list's "Unpaid" view. */
export const UNPAID = ["pending", "authorized", "partially_paid"];

/** Statuses where money was collected, at least partly. */
export const COLLECTED = ["paid", "partially_paid", "partially_refunded"];
