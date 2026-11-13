"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";

export type Result = { error?: string };

/** Adds a supplier and opens its page. */
export async function createSupplier(
  input: AdminSchemas["CreateSupplierRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/suppliers", { body: input });
  if (!data) {
    const t = await getT();
    return { error: error ? await describe(error) : t("suppliers.create_failed") };
  }
  revalidatePath("/suppliers");
  redirect(`/suppliers/${data.id}`);
}

/** A JSON Merge Patch of a supplier: absent keeps a field, `null` clears it. */
export type SupplierPatch = AdminSchemas["UpdateSupplierRequest"];

export async function updateSupplier(id: string, patch: SupplierPatch): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/suppliers/{id}", {
    params: { path: { id } },
    body: patch,
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/suppliers/${id}`);
  return {};
}

export async function deleteSupplier(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/suppliers/{id}", { params: { path: { id } } });
  if (error) return { error: await describe(error) };
  revalidatePath("/suppliers");
  redirect("/suppliers");
}

/** Sets what the supplier charges for a variant, replacing what it had. */
export async function setCatalogueRow(
  id: string,
  variant: string,
  row: AdminSchemas["SetSupplierVariantRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/suppliers/{id}/variants/{variant_id}", {
    params: { path: { id, variant_id: variant } },
    body: row,
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/suppliers/${id}`);
  return {};
}

export async function removeCatalogueRow(id: string, variant: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/suppliers/{id}/variants/{variant_id}", {
    params: { path: { id, variant_id: variant } },
  });
  if (error) return { error: await describe(error) };
  revalidatePath(`/suppliers/${id}`);
  return {};
}
