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

const PAYMENT: Record<
  AdminSchemas["FinancialStatusDto"],
  { label: string; variant: "success" | "warning" }
> = {
  paid: { label: "Paid", variant: "success" },
  pending: { label: "Payment pending", variant: "warning" },
};

export function PaymentBadge({ status }: { status: AdminSchemas["FinancialStatusDto"] }) {
  const { label, variant } = PAYMENT[status];
  return <Badge label={label} variant={variant} />;
}

// The SDK types open enums as strings (a new status mustn't break old admins), so an
// unknown one shows as it is.
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
