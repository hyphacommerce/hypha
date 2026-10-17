"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/** The categories pages, and the product pages that label categories. */
function refresh(id?: string) {
  revalidatePath("/products/categories");
  if (id) revalidatePath(`/products/categories/${id}`);
  revalidatePath("/products");
}

/** Creates a category and opens it. */
export async function createCategory(input: {
  name: string;
  handle: string;
  parent: string | null;
  visible: boolean;
}): Promise<Result> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/categories", { body: input });
  if (error) return { error: describe(error) };
  refresh();
  redirect(`/products/categories/${data.id}`);
}

/** A Merge Patch of a category: only the fields given change. */
export async function updateCategory(
  id: string,
  body: AdminSchemas["UpdateCategoryRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/categories/{id}", {
    params: { path: { id } },
    body,
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

/** Deletes a category without subcategories, then back to the tree. */
export async function deleteCategory(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/categories/{id}", { params: { path: { id } } });
  if (error) return { error: describe(error) };
  refresh();
  redirect("/products/categories");
}

export async function addCategoryProducts(id: string, products: string[]): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/categories/{id}/products", {
    params: { path: { id } },
    body: { products },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

export async function removeCategoryProduct(id: string, product: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/categories/{id}/products", {
    params: { path: { id } },
    body: { products: [product] },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

/** Sets the category's image, or clears it with `null`. */
export async function setCategoryImage(id: string, image: string | null): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/categories/{id}/image", {
    params: { path: { id } },
    body: { image },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  revalidatePath("/media");
  return {};
}
