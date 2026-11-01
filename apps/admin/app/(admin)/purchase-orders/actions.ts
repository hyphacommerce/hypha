"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";

export type Result = { error?: string };

/** Writes a draft purchase order and opens it. */
export async function createPurchaseOrder(
  input: AdminSchemas["CreatePurchaseOrderRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/purchase-orders", { body: input });
  if (!data) return { error: error ? describe(error) : "Couldn't write the purchase order." };
  revalidatePath("/purchase-orders");
  redirect(`/purchase-orders/${data.id}`);
}

/** A JSON Merge Patch of a purchase order: absent keeps a field, `null` clears it. */
export type PurchaseOrderPatch = AdminSchemas["UpdatePurchaseOrderRequest"];

export async function updatePurchaseOrder(id: string, patch: PurchaseOrderPatch): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/purchase-orders/{id}", {
    params: { path: { id } },
    body: patch,
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/purchase-orders/${id}`);
  return {};
}

/** Places, closes or cancels a purchase order. */
export async function movePurchaseOrder(
  id: string,
  action: "order" | "close" | "cancel",
): Promise<Result> {
  const client = await sessionClient();
  const params = { params: { path: { id } } };
  const { error } =
    action === "order"
      ? await client.POST("/admin/purchase-orders/{id}/order", params)
      : action === "close"
        ? await client.POST("/admin/purchase-orders/{id}/close", params)
        : await client.POST("/admin/purchase-orders/{id}/cancel", params);
  if (error) return { error: describe(error) };
  revalidatePath(`/purchase-orders/${id}`);
  revalidatePath("/purchase-orders");
  return {};
}

/**
 * Emails a purchase order to its supplier (a draft is ordered first): `to` blank sends to
 * the supplier's primary contact.
 */
export async function sendPurchaseOrder(
  id: string,
  input: { to: string[]; cc: string[]; message: string },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/purchase-orders/{id}/send", {
    params: { path: { id } },
    body: {
      ...(input.to.length > 0 ? { to: input.to } : {}),
      cc: input.cc,
      ...(input.message.trim() ? { message: input.message } : {}),
    },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/purchase-orders/${id}`);
  revalidatePath("/purchase-orders");
  return {};
}

/** Deletes a draft and goes back to the list. */
export async function deletePurchaseOrder(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/purchase-orders/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/purchase-orders");
  redirect("/purchase-orders");
}

export type ReceiveResult = Result & {
  /** Variants given an untracked stock record by this receipt. */
  newItems?: string[];
};

/**
 * Records goods arriving. `key` is made once per dialog, so a double submit records one
 * receipt (the API replays the first).
 */
export async function receivePurchaseOrder(
  id: string,
  input: {
    key: string;
    location: string;
    lines: { variant: string; accepted: number; rejected: number }[];
    allowOver: boolean;
    note: string;
  },
): Promise<ReceiveResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/purchase-orders/{id}/receipts", {
    params: { path: { id }, header: { "Idempotency-Key": input.key } },
    body: {
      location: input.location,
      lines: input.lines,
      allow_over: input.allowOver,
      note: input.note.trim() || null,
    },
  });
  if (!data) return { error: error ? describe(error) : "Couldn't record the receipt." };
  revalidatePath(`/purchase-orders/${id}`);
  revalidatePath("/purchase-orders");
  return { newItems: data.new_items };
}

/** Takes a receipt back: its units come off stock, its counts off the order. */
export async function cancelReceipt(purchaseOrder: string, receipt: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/receipts/{id}/cancel", {
    params: { path: { id: receipt } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/purchase-orders/${purchaseOrder}`);
  revalidatePath("/purchase-orders");
  return {};
}

/** Turns stock tracking on for variants a receipt gave a record, so their stock sells. */
export async function trackVariants(variants: string[]): Promise<Result> {
  const client = await sessionClient();
  for (const variant of variants) {
    const { data } = await client.GET("/admin/variants/{id}/inventory", {
      params: { path: { id: variant } },
    });
    if (!data?.id) continue;
    const { error } = await client.PATCH("/admin/inventory-items/{id}", {
      params: { path: { id: data.id } },
      body: { tracked: true },
    });
    if (error) return { error: describe(error) };
  }
  revalidatePath("/inventory");
  return {};
}
