"use server";

// Bulk actions on a list's selected rows (design note 0024, section 6). Where the API
// has a set-based route (publications, category membership) one call does them all;
// otherwise each row is its own call, a few at a time, and every failure is reported
// with the row it belongs to.

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** How a bulk action went: rows changed, rows already so, and why the others failed. */
export type BulkResult = {
  changed: number;
  unchanged: number;
  failed: { id: string; label: string; error: string }[];
  error?: string;
};

type Item = { id: string; label: string };

/** Runs `each` over `items`, `size` at a time. */
async function inBatches<T>(items: T[], size: number, each: (item: T) => Promise<void>) {
  for (let i = 0; i < items.length; i += size) {
    await Promise.all(items.slice(i, i + size).map(each));
  }
}

/**
 * Sets the status of each product that isn't in it already. A transition the product's
 * status doesn't allow (a draft straight to archived) fails for that product alone.
 */
export async function setProductsStatus(
  products: (Item & { status: AdminSchemas["ProductStatus"] })[],
  status: AdminSchemas["ProductStatus"],
): Promise<BulkResult> {
  const client = await sessionClient();
  const result: BulkResult = { changed: 0, unchanged: 0, failed: [] };
  const todo = products.filter((p) => p.status !== status);
  result.unchanged = products.length - todo.length;
  await inBatches(todo, 4, async (p) => {
    const { error } = await client.PATCH("/admin/products/{id}/status", {
      params: { path: { id: p.id } },
      body: { status },
    });
    if (error) result.failed.push({ id: p.id, label: p.label, error: describe(error) });
    else result.changed += 1;
  });
  revalidatePath("/products");
  return result;
}

/** Publishes the products to `channel`, or takes them off it: one call either way. */
export async function setProductsPublished(
  channel: string,
  products: Item[],
  publish: boolean,
): Promise<BulkResult> {
  const client = await sessionClient();
  const ids = products.map((p) => p.id);
  const options = { params: { path: { id: channel } }, body: { products: ids } };
  const { error } = publish
    ? await client.POST("/admin/channels/{id}/products", options)
    : await client.DELETE("/admin/channels/{id}/products", options);
  revalidatePath("/products");
  if (error) return { changed: 0, unchanged: 0, failed: [], error: describe(error) };
  return { changed: ids.length, unchanged: 0, failed: [] };
}

/** Adds the products to `category`, or removes them from it: one call either way. */
export async function setProductsInCategory(
  category: string,
  products: Item[],
  add: boolean,
): Promise<BulkResult> {
  const client = await sessionClient();
  const ids = products.map((p) => p.id);
  const options = { params: { path: { id: category } }, body: { products: ids } };
  const { error } = add
    ? await client.POST("/admin/categories/{id}/products", options)
    : await client.DELETE("/admin/categories/{id}/products", options);
  revalidatePath("/products");
  revalidatePath(`/products/categories/${category}`);
  if (error) return { changed: 0, unchanged: 0, failed: [], error: describe(error) };
  return { changed: ids.length, unchanged: 0, failed: [] };
}

/** Marks each pending, uncancelled order paid; the others are left as they are. */
export async function markOrdersPaid(
  orders: (Item & { pending: boolean })[],
): Promise<BulkResult> {
  const client = await sessionClient();
  const result: BulkResult = { changed: 0, unchanged: 0, failed: [] };
  const todo = orders.filter((o) => o.pending);
  result.unchanged = orders.length - todo.length;
  await inBatches(todo, 4, async (o) => {
    const { error } = await client.POST("/admin/orders/{id}/paid", {
      params: { path: { id: o.id } },
    });
    if (error) result.failed.push({ id: o.id, label: o.label, error: describe(error) });
    else result.changed += 1;
  });
  revalidatePath("/orders");
  return result;
}
