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
 * Whether staff can mark the order paid, by the API's rule (design note 0027, §5): not
 * cancelled, no `authorized` payment (only its provider can capture it), and either a
 * `pending` payment to record or, with no open payment, a `pending` order (the API then
 * records a manual payment of the total). Any other status means money moved already
 * (`order_not_payable`). Without its payments (a list's summary), only `pending` orders.
 */
export function canMarkPaid(order: {
  financial_status: string;
  cancelled_at?: string | null;
  payments?: { status: string }[];
}): boolean {
  if (order.cancelled_at) return false;
  const payments = order.payments ?? [];
  if (payments.some((p) => p.status === "authorized")) return false;
  if (payments.some((p) => p.status === "pending")) return true;
  return order.financial_status === "pending";
}

/** Statuses with money still to collect: the orders list's "Unpaid" view. */
export const UNPAID = ["pending", "authorized", "partially_paid"];

/** Statuses where money was collected, at least partly. */
export const COLLECTED = ["paid", "partially_paid", "partially_refunded"];
