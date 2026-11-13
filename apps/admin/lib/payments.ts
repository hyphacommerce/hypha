// Payments in the admin: the method presets staff start from, and which orders staff
// can still mark paid. Plain data and functions over a translator, for server and client
// code alike.

import type { TFor } from "./messages";

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

/** The name a method made from each preset starts with. */
const PRESET_NAMES: Record<PaymentPreset["value"], string> = {
  cash_on_delivery: "Cash on delivery",
  bank_transfer: "Bank transfer",
  custom: "",
};

/** The presets, in the order offered. */
const PRESET_VALUES = ["cash_on_delivery", "bank_transfer", "custom"] as const;

/**
 * The presets as `t`'s language words them. The name a method starts with is content
 * (shoppers see it at checkout), which is in the store's language whatever the staff
 * member's is, so it isn't translated: staff change it before saving.
 */
export function paymentPresets(t: TFor<"payments">): PaymentPreset[] {
  return PRESET_VALUES.map((value) => ({
    value,
    label: t(`payments.preset_${value}_label`),
    description: t(`payments.preset_${value}_description`),
    name: PRESET_NAMES[value],
    instructionsPlaceholder: t(`payments.preset_${value}_instructions`),
  }));
}

/** A provider a method can name besides `manual`: an installed app answering
 * `payments.start`, or a spore built into the API. */
export type PaymentApp = { key: string; name: string; isBuiltIn: boolean };

/**
 * What a provider key reads as: "Manual", or the app's or spore's name from its key
 * (`app_stripe`, `spore_stripe`).
 */
export function providerLabel(provider: string, t: TFor<"payments">): string {
  if (provider === "manual") return t("payments.provider_manual");
  for (const prefix of ["app_", "spore_"])
    if (provider.startsWith(prefix)) return provider.slice(prefix.length);
  return provider;
}

/**
 * Whether staff can mark the order paid, by the API's rule: not cancelled, no
 * `authorized` payment (only its provider can capture it), and either a `pending`
 * payment to record or, with no open payment, a `pending` order (the API then records a
 * manual payment of the total). Any other status means money moved already
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
