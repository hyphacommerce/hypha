// Badges for the API's statuses, one place so every page colours them alike.

import { Badge } from "@astryxdesign/core/Badge";
import type { AdminSchemas } from "@hypha/sdk/admin";

const PRODUCT: Record<
  AdminSchemas["ProductStatus"],
  { label: string; variant: "success" | "neutral" | "warning" }
> = {
  active: { label: "Active", variant: "success" },
  draft: { label: "Draft", variant: "neutral" },
  archived: { label: "Archived", variant: "warning" },
};

export function ProductStatusBadge({ status }: { status: AdminSchemas["ProductStatus"] }) {
  const { label, variant } = PRODUCT[status];
  return <Badge label={label} variant={variant} />;
}

// The SDK types open enums as strings (a new status mustn't break old admins), so an
// unknown one shows as it is.
type Variant = "success" | "neutral" | "warning" | "info" | "error";

/** An order's payment status (design note 0027, PA7). */
const PAYMENT: Record<string, { label: string; variant: Variant }> = {
  pending: { label: "Payment pending", variant: "warning" },
  authorized: { label: "Authorized", variant: "info" },
  partially_paid: { label: "Partly paid", variant: "warning" },
  paid: { label: "Paid", variant: "success" },
  partially_refunded: { label: "Partly refunded", variant: "neutral" },
  refunded: { label: "Refunded", variant: "neutral" },
  voided: { label: "Voided", variant: "neutral" },
};

export function PaymentBadge({ status }: { status: AdminSchemas["FinancialStatusDto"] }) {
  const { label, variant } = PAYMENT[status] ?? { label: status, variant: "neutral" };
  return <Badge label={label} variant={variant} />;
}

/** One payment's status, on an order's payment card. */
const ONE_PAYMENT: Record<string, { label: string; variant: Variant }> = {
  pending: { label: "Pending", variant: "warning" },
  authorized: { label: "Authorized", variant: "info" },
  paid: { label: "Paid", variant: "success" },
  failed: { label: "Failed", variant: "error" },
  voided: { label: "Voided", variant: "neutral" },
  partially_refunded: { label: "Partly refunded", variant: "neutral" },
  refunded: { label: "Refunded", variant: "neutral" },
};

export function PaymentStatusBadge({ status }: { status: AdminSchemas["PaymentStatusDto"] }) {
  const { label, variant } = ONE_PAYMENT[status] ?? { label: status, variant: "neutral" };
  return <Badge label={label} variant={variant} />;
}

const FULFILLMENT: Record<string, { label: string; variant: "success" | "neutral" | "info" }> = {
  unfulfilled: { label: "Unfulfilled", variant: "neutral" },
  partially_fulfilled: { label: "Partly fulfilled", variant: "info" },
  fulfilled: { label: "Fulfilled", variant: "success" },
};

export function FulfillmentBadge({
  status,
}: {
  status: AdminSchemas["FulfillmentStatusDto"];
}) {
  const { label, variant } = FULFILLMENT[status] ?? { label: status, variant: "neutral" };
  return <Badge label={label} variant={variant} />;
}

export function CancelledBadge() {
  return <Badge label="Cancelled" variant="error" />;
}

/** A shipment: shipped, or taken back. */
export function ShipmentBadge({ status }: { status: AdminSchemas["FulfillmentStateDto"] }) {
  return status === "cancelled" ? (
    <Badge label="Taken back" variant="warning" />
  ) : (
    <Badge label="Shipped" variant="success" />
  );
}
