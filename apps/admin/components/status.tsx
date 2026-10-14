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

const FULFILLMENT: Record<AdminSchemas["FulfillmentStatusDto"], string> = {
  unfulfilled: "Unfulfilled",
};

export function FulfillmentBadge({
  status,
}: {
  status: AdminSchemas["FulfillmentStatusDto"];
}) {
  return <Badge label={FULFILLMENT[status]} variant="neutral" />;
}
