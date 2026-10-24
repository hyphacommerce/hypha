// How the usage tab names an order and a customer from a usage record.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import { customerName } from "../format.ts";

type Usage = AdminSchemas["DiscountUsageResponse"];

/** "#1001", or the order's ID when its number isn't known. */
export function orderLabel(u: Pick<Usage, "order" | "order_number">): string {
  return u.order_number == null ? u.order : `#${u.order_number}`;
}

/**
 * The customer's name, else their email, else their ID: a caller without the
 * `customer_data:*` scopes gets neither (they're listed in `redacted`).
 */
export function customerLabel(
  u: Pick<Usage, "customer" | "first_name" | "last_name" | "email">,
): string {
  return customerName(u) || u.email || u.customer;
}
