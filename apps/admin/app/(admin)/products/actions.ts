"use server";

import type { AdminSchemas, ApiError } from "@hypha/sdk/admin";
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

/** The outcome of an edit: a message to show, and the API's code and details for the
 * cases a form handles itself (removing variants with an option value). */
export type EditResult = { error?: string; code?: string; details?: unknown };

function failed(error: ApiError): EditResult {
  return { error: describe(error), code: error.code, details: error.details };
}

/** Edits change the product page; titles, handles and media also show in the list. */
function refresh(id: string) {
  revalidatePath(`/products/${id}`);
  revalidatePath("/products");
}

export async function updateProduct(
  id: string,
  input: AdminSchemas["UpdateProductRequest"],
): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/products/{id}", {
    params: { path: { id } },
    body: input,
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

export type OptionDraft = AdminSchemas["OptionRequest"];

export async function setOptions(
  id: string,
  options: OptionDraft[],
  removeVariants: string[] = [],
): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/products/{id}/options", {
    params: { path: { id } },
    body: { options, remove_variants: removeVariants },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

export async function addVariant(
  id: string,
  input: { sku: string; price: string; currency: AdminSchemas["Currency"]; selections: string[] },
): Promise<EditResult> {
  const amount = parseMoney(input.price, input.currency);
  if (amount === null) return { error: "Enter the price as a number, like 18 or 18.50." };
  const client = await sessionClient();
  const { error } = await client.POST("/admin/products/{id}/variants", {
    params: { path: { id } },
    body: {
      sku: input.sku.trim() === "" ? null : input.sku,
      price: { amount, currency: input.currency },
      selections: input.selections,
    },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

/** Grams from what someone typed: a whole number, or null for a blank (not entered). */
function parseGrams(input: string): number | null | undefined {
  const text = input.trim();
  if (text === "") return null;
  if (!/^\d+$/.test(text)) return undefined;
  const grams = Number(text);
  return grams <= 4_294_967_295 ? grams : undefined;
}

export async function updateVariant(
  id: string,
  variant: string,
  input: {
    sku: string;
    price: string;
    currency: AdminSchemas["Currency"];
    weight: string;
    requiresShipping: boolean;
  },
): Promise<EditResult> {
  const amount = parseMoney(input.price, input.currency);
  if (amount === null) return { error: "Enter the price as a number, like 18 or 18.50." };
  const grams = parseGrams(input.weight);
  if (grams === undefined) return { error: "Enter the weight in whole grams, like 350." };
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/products/{id}/variants/{variant_id}", {
    params: { path: { id, variant_id: variant } },
    // A blank SKU or weight clears it (`null` in a Merge Patch).
    body: {
      sku: input.sku.trim() === "" ? null : input.sku,
      price: { amount, currency: input.currency },
      weight_grams: grams,
      requires_shipping: input.requiresShipping,
    },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

export async function removeVariant(id: string, variant: string): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/products/{id}/variants/{variant_id}", {
    params: { path: { id, variant_id: variant } },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

/** Attaches, reorders and detaches in one: `assets` becomes the whole list. */
export async function setProductMedia(id: string, assets: string[]): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/products/{id}/media", {
    params: { path: { id } },
    body: { assets },
  });
  if (error) return failed(error);
  refresh(id);
  revalidatePath("/media");
  return {};
}

export async function setVariantMedia(
  id: string,
  variant: string,
  asset: string | null,
): Promise<EditResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/products/{id}/variants/{variant_id}/media", {
    params: { path: { id, variant_id: variant } },
    body: { asset },
  });
  if (error) return failed(error);
  refresh(id);
  return {};
}

/**
 * A page of the library's images and videos, for picking media for a product. The
 * library lists one kind or all, so PDFs are dropped here; pages of only PDFs are read
 * past (a few at most), so a page is empty only when the library has nothing more.
 */
export async function libraryPage(
  cursor?: string,
): Promise<{ items: AdminSchemas["AssetResponse"][]; next?: string } | { error: string }> {
  const client = await sessionClient();
  let next = cursor;
  for (let read = 0; read < 5; read++) {
    const { data, error } = await client.GET("/admin/assets", {
      params: { query: { cursor: next, limit: 60 } },
    });
    if (error) return { error: describe(error) };
    const items = data.items.filter((a) => a.kind !== "file");
    next = data.next_cursor ?? undefined;
    if (items.length > 0 || !next) return { items, next };
  }
  return { items: [], next };
}
