"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";
import { type TrackingInput, trackingBody, trackingProblem } from "@/lib/tracking";

export type ActionResult = { error?: string };

/**
 * Records money staff collected themselves: the order's open payment becomes paid
 * with the `reference`, or the API records a manual one of the total when the order
 * has none.
 */
export async function markOrderPaid(id: string, reference: string): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/orders/{id}/paid", {
    params: { path: { id } },
    body: { reference: reference.trim() === "" ? null : reference.trim() },
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
    tracking: TrackingInput[];
    notify: boolean;
  },
): Promise<ActionResult> {
  const problem = trackingProblem(input.tracking);
  if (problem) return { error: problem };
  const client = await sessionClient();
  const { error } = await client.POST("/admin/orders/{id}/fulfillments", {
    params: { path: { id: orderId }, header: { "Idempotency-Key": input.key } },
    body: {
      location: input.location,
      lines: input.lines,
      tracking: trackingBody(input.tracking),
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
  input: TrackingInput[],
  notify: boolean,
): Promise<ActionResult> {
  const problem = trackingProblem(input);
  if (problem) return { error: problem };
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/fulfillments/{id}", {
    params: { path: { id: fulfillmentId } },
    body: { tracking: trackingBody(input), notify },
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

/**
 * Sends one of the order's emails again: a new message with the template and the order
 * as they are now, to the same recipient.
 */
export async function resendNotification(orderId: string, id: string): Promise<ActionResult> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/notifications/{id}/resend", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/orders/${orderId}`);
  return {};
}
