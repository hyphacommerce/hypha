// Badges for the API's statuses, one place so every page colours them alike. Each takes
// the caller's translator (`getT()` on the server, `useT()` in a client component), so
// server and client pages render the same badge, its text from the staff member's catalog.

import { Badge } from "@astryxdesign/core/Badge";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import type { KeyIn, TFor } from "@/lib/messages";

type Variant = "success" | "neutral" | "warning" | "info" | "error";

/** A status as the admin shows it: its label's message key and its colour. */
type Look = { label: KeyIn<"components">; variant: Variant };

/** The caller's translator: a server page's or a client component's. */
type Translate = TFor<"components">;

const PRODUCT: Record<AdminSchemas["ProductStatus"], Look> = {
  active: { label: "components.status_product_active", variant: "success" },
  draft: { label: "components.status_product_draft", variant: "neutral" },
  archived: { label: "components.status_product_archived", variant: "warning" },
};

export function ProductStatusBadge({
  status,
  t,
}: {
  status: AdminSchemas["ProductStatus"];
  t: Translate;
}) {
  const { label, variant } = PRODUCT[status];
  return <Badge label={t(label)} variant={variant} />;
}

/** The look of `status` among `looks`, or none for one this build doesn't know. */
function lookOf(looks: Record<string, Look>, status: string): Look | undefined {
  return Object.hasOwn(looks, status) ? looks[status] : undefined;
}

/**
 * The badge of a status in `looks`. The SDK types open enums as strings (a new status
 * mustn't break old admins), so an unknown one shows as it is.
 */
function StatusBadge({
  looks,
  status,
  t,
}: {
  looks: Record<string, Look>;
  status: string;
  t: Translate;
}) {
  const look = lookOf(looks, status);
  return look ? (
    <Badge label={t(look.label)} variant={look.variant} />
  ) : (
    <Badge label={status} variant="neutral" />
  );
}

/** An order's payment status. */
const PAYMENT: Record<string, Look> = {
  pending: { label: "components.status_financial_pending", variant: "warning" },
  authorized: { label: "components.status_financial_authorized", variant: "info" },
  partially_paid: { label: "components.status_financial_partially_paid", variant: "warning" },
  paid: { label: "components.status_financial_paid", variant: "success" },
  partially_refunded: {
    label: "components.status_financial_partially_refunded",
    variant: "neutral",
  },
  refunded: { label: "components.status_financial_refunded", variant: "neutral" },
  voided: { label: "components.status_financial_voided", variant: "neutral" },
};

export function PaymentBadge({
  status,
  t,
}: {
  status: AdminSchemas["FinancialStatusDto"];
  t: Translate;
}) {
  return <StatusBadge looks={PAYMENT} status={status} t={t} />;
}

/** One payment's status, on an order's payment card. */
const ONE_PAYMENT: Record<string, Look> = {
  pending: { label: "components.status_payment_pending", variant: "warning" },
  authorized: { label: "components.status_payment_authorized", variant: "info" },
  paid: { label: "components.status_payment_paid", variant: "success" },
  failed: { label: "components.status_payment_failed", variant: "error" },
  voided: { label: "components.status_payment_voided", variant: "neutral" },
  partially_refunded: {
    label: "components.status_payment_partially_refunded",
    variant: "neutral",
  },
  refunded: { label: "components.status_payment_refunded", variant: "neutral" },
};

export function PaymentStatusBadge({
  status,
  t,
}: {
  status: AdminSchemas["PaymentStatusDto"];
  t: Translate;
}) {
  return <StatusBadge looks={ONE_PAYMENT} status={status} t={t} />;
}

const FULFILLMENT: Record<string, Look> = {
  unfulfilled: { label: "components.status_fulfillment_unfulfilled", variant: "neutral" },
  partially_fulfilled: {
    label: "components.status_fulfillment_partially_fulfilled",
    variant: "info",
  },
  fulfilled: { label: "components.status_fulfillment_fulfilled", variant: "success" },
};

export function FulfillmentBadge({
  status,
  t,
}: {
  status: AdminSchemas["FulfillmentStatusDto"];
  t: Translate;
}) {
  return <StatusBadge looks={FULFILLMENT} status={status} t={t} />;
}

export function CancelledBadge({ t }: { t: Translate }) {
  return <Badge label={t("components.status_order_cancelled")} variant="error" />;
}

/** An order whose shipping address staff should look at before shipping it. */
export function NeedsReviewBadge({ t }: { t: Translate }) {
  return <Badge label={t("components.status_order_needs_review")} variant="warning" />;
}

/** A shipment: shipped, or taken back. */
export function ShipmentBadge({
  status,
  t,
}: {
  status: AdminSchemas["FulfillmentStateDto"];
  t: Translate;
}) {
  return status === "cancelled" ? (
    <Badge label={t("components.status_shipment_cancelled")} variant="warning" />
  ) : (
    <Badge label={t("components.status_shipment_shipped")} variant="success" />
  );
}

/** A purchase order's status. */
const PURCHASE_ORDER: Record<string, Look> = {
  draft: { label: "components.status_purchase_order_draft", variant: "neutral" },
  ordered: { label: "components.status_purchase_order_ordered", variant: "info" },
  closed: { label: "components.status_purchase_order_closed", variant: "success" },
  cancelled: { label: "components.status_purchase_order_cancelled", variant: "warning" },
};

export function PurchaseOrderStatusBadge({
  status,
  t,
}: {
  status: AdminSchemas["PurchaseOrderStatusDto"];
  t: Translate;
}) {
  return <StatusBadge looks={PURCHASE_ORDER} status={status} t={t} />;
}

/** How much of a purchase order has arrived. */
const RECEIPT: Record<string, Look> = {
  none: { label: "components.status_receipt_none", variant: "neutral" },
  partial: { label: "components.status_receipt_partial", variant: "warning" },
  received: { label: "components.status_receipt_received", variant: "success" },
};

export function ReceiptStatusBadge({
  status,
  t,
}: {
  status: AdminSchemas["ReceiptStatusDto"];
  t: Translate;
}) {
  return <StatusBadge looks={RECEIPT} status={status} t={t} />;
}

/** A transfer's status. */
const TRANSFER: Record<string, Look> = {
  draft: { label: "components.status_transfer_draft", variant: "neutral" },
  in_transit: { label: "components.status_transfer_in_transit", variant: "info" },
  received: { label: "components.status_transfer_received", variant: "success" },
  cancelled: { label: "components.status_transfer_cancelled", variant: "warning" },
};

export function TransferStatusBadge({
  status,
  t,
}: {
  status: AdminSchemas["TransferStatusDto"];
  t: Translate;
}) {
  return <StatusBadge looks={TRANSFER} status={status} t={t} />;
}

/** The statuses that have a badge, by family. */
const FAMILIES = {
  product: PRODUCT,
  financial: PAYMENT,
  fulfillment: FULFILLMENT,
  purchase_order: PURCHASE_ORDER,
  receipt: RECEIPT,
  transfer: TRANSFER,
} satisfies Record<string, Record<string, Look>>;

/**
 * A status as its badge says it, for text beside one: a filter's option, a view's tab.
 * One source for both, so a filter and the badges it filters can't disagree. An unknown
 * status as it is.
 */
export function statusLabel(family: keyof typeof FAMILIES, status: string, t: Translate): string {
  const look = lookOf(FAMILIES[family], status);
  return look ? t(look.label) : status;
}

/** A filter's options for `statuses`, in the order given, each named as its badge is. */
export function statusOptions<S extends string>(
  family: keyof typeof FAMILIES,
  statuses: readonly S[],
  t: Translate,
): { value: S; label: string }[] {
  return statuses.map((value) => ({ value, label: statusLabel(family, value, t) }));
}
