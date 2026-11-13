"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";

export type Result = { error?: string };

/** The store's locations, for choosing where a transfer goes from and to. */
export async function transferLocations(): Promise<{ id: string; name: string }[]> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/locations");
  return (data?.items ?? []).map((l) => ({ id: l.id, name: l.name }));
}

/** Writes a draft transfer and opens it. */
export async function createTransfer(
  input: AdminSchemas["CreateTransferRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/transfers", { body: input });
  if (!data) {
    const t = await getT();
    return { error: error ? await describe(error) : t("transfers.create_failed") };
  }
  revalidatePath("/transfers");
  redirect(`/transfers/${data.id}`);
}

/** A JSON Merge Patch of a draft transfer: absent keeps a field, `null` clears it. */
export type TransferPatch = AdminSchemas["UpdateTransferRequest"];

export async function updateTransfer(id: string, patch: TransferPatch): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/transfers/{id}", {
    params: { path: { id } },
    body: patch,
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/transfers/${id}`);
  return {};
}

/**
 * Ships a draft whole. `key` is made once per dialog, so a double submit ships once (the
 * API replays the first).
 */
export async function shipTransfer(id: string, key: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/transfers/{id}/ship", {
    params: { path: { id }, header: { "Idempotency-Key": key } },
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/transfers/${id}`);
  revalidatePath("/transfers");
  revalidatePath("/inventory");
  return {};
}

/** Records units arriving at the destination, once per `key` like `shipTransfer`. */
export async function receiveTransfer(
  id: string,
  input: { key: string; lines: { variant: string; accepted: number; rejected: number }[] },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/transfers/{id}/receive", {
    params: { path: { id }, header: { "Idempotency-Key": input.key } },
    body: { lines: input.lines },
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/transfers/${id}`);
  revalidatePath("/transfers");
  revalidatePath("/inventory");
  return {};
}

/** Calls off a draft. */
export async function cancelTransfer(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/transfers/{id}/cancel", {
    params: { path: { id } },
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/transfers/${id}`);
  revalidatePath("/transfers");
  return {};
}

/** Deletes a draft and goes back to the list. */
export async function deleteTransfer(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/transfers/{id}", { params: { path: { id } } });
  if (error) return { error: await describe(error) };
  revalidatePath("/transfers");
  redirect("/transfers");
}
