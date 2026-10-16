"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

export type ActionResult = { error?: string };

/** Records that the order was paid outside Hypha (no payment spores yet, 0007 §10). */
export async function markOrderPaid(id: string): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/orders/{id}/paid", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/orders/${id}`);
  revalidatePath("/orders");
  return {};
}

function refresh(orderId: string) {
  revalidatePath(`/orders/${orderId}`);
  revalidatePath("/orders");
}

export type TrackingInput = { carrier: string; number: string; url: string };

/** One tracking number, or none when no number was entered. */
function tracking(input: TrackingInput) {
  if (!input.number.trim()) return [];
  return [
    {
      number: input.number,
      carrier: input.carrier.trim() || null,
      url: input.url.trim() || null,
    },
  ];
}

/**
 * Records a shipment. `key` is made once per dialog, so a double submit records it
 * once (the API replays the first).
 */
export async function fulfilOrder(
  orderId: string,
  input: {
    key: string;
    location: string;
    lines: { line: string; quantity: number }[];
    tracking: TrackingInput;
    notify: boolean;
  },
): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/orders/{id}/fulfillments", {
    params: { path: { id: orderId }, header: { "Idempotency-Key": input.key } },
    body: {
      location: input.location,
      lines: input.lines,
      tracking: tracking(input.tracking),
      notify: input.notify,
    },
  });
  if (error) return { error: describe(error) };
  refresh(orderId);
  return {};
}

export async function updateTracking(
  orderId: string,
  fulfillmentId: string,
  input: TrackingInput,
  notify: boolean,
): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/fulfillments/{id}", {
    params: { path: { id: fulfillmentId } },
    body: { tracking: tracking(input), notify },
  });
  if (error) return { error: describe(error) };
  refresh(orderId);
  return {};
}

/** Takes a shipment back: its units are reserved for the order again. */
export async function takeBack(orderId: string, fulfillmentId: string): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/fulfillments/{id}/cancel", {
    params: { path: { id: fulfillmentId } },
  });
  if (error) return { error: describe(error) };
  refresh(orderId);
  return {};
}

export async function cancelOrder(
  orderId: string,
  reason: string,
  notify: boolean,
): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/orders/{id}/cancel", {
    params: { path: { id: orderId } },
    body: { reason, notify },
  });
  if (error) return { error: describe(error) };
  refresh(orderId);
  return {};
}
