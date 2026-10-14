"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { parseMoney } from "@/lib/format";
import { describe, sessionClient } from "@/lib/hypha";

export type FormState = { error?: string };

export async function createProduct(
  _previous: FormState,
  input: { title: string; handle: string; price: string; currency: AdminSchemas["Currency"] },
): Promise<FormState> {
  const amount = parseMoney(input.price, input.currency);
  if (amount === null) return { error: "Enter the price as a number, like 18 or 18.50." };
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/products", {
    body: {
      title: input.title,
      handle: input.handle,
      price: { amount, currency: input.currency },
    },
  });
  if (error) return { error: describe(error) };
  revalidatePath("/products");
  redirect(`/products/${data.id}`);
}

export async function setProductStatus(
  id: string,
  status: AdminSchemas["ProductStatus"],
): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/products/{id}/status", {
    params: { path: { id } },
    body: { status },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/products/${id}`);
  revalidatePath("/products");
  return {};
}
