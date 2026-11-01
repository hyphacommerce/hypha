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
