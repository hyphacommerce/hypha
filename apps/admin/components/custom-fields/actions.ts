"use server";

// What the custom fields cards and inputs ask the API: an owner's values, writing
// them, and naming a reference's target.

import { revalidatePath } from "next/cache";

import { flatten } from "@/lib/categories";
import { type FieldsResponse, type OwnerType, jsonObject, problemsOf } from "@/lib/custom-fields";
import { customerName } from "@/lib/format";
import { describe, sessionClient } from "@/lib/hypha";

export type FieldsResult = {
  fields?: FieldsResponse;
  error?: string;
  /** Problems by field key, shown next to their inputs. */
  problems?: Record<string, string>;
};

export type FieldsPatch = {
  custom_fields: Record<string, unknown>;
  if_versions: Record<string, number>;
};

/** The pages showing an owner, refreshed after its fields change. */
const PAGES: Record<OwnerType, (id: string) => string | null> = {
  product: (id) => `/products/${id}`,
  variant: () => null,
  category: (id) => `/products/categories/${id}`,
  customer: (id) => `/customers/${id}`,
  order: (id) => `/orders/${id}`,
};

async function read(
  client: Awaited<ReturnType<typeof sessionClient>>,
  owner: OwnerType,
  id: string,
) {
  const path = { params: { path: { id } } };
  switch (owner) {
    case "product":
      return client.GET("/admin/products/{id}/custom-fields", path);
    case "variant":
      return client.GET("/admin/variants/{id}/custom-fields", path);
    case "category":
      return client.GET("/admin/categories/{id}/custom-fields", path);
    case "customer":
      return client.GET("/admin/customers/{id}/custom-fields", path);
    case "order":
      return client.GET("/admin/orders/{id}/custom-fields", path);
  }
}

/** An owner's values as the session sees them, with their versions. */
export async function loadCustomFields(owner: OwnerType, id: string): Promise<FieldsResult> {
  const client = await sessionClient();
  const { data, error } = await read(client, owner, id);
  return error ? { error: describe(error) } : { fields: data };
}

/** Writes `patch` (a JSON Merge Patch of the values) to the owner's fields. */
export async function saveCustomFields(
  owner: OwnerType,
  id: string,
  patch: FieldsPatch,
): Promise<FieldsResult> {
  const client = await sessionClient();
  const request = {
    params: { path: { id } },
    body: { custom_fields: jsonObject(patch.custom_fields), if_versions: patch.if_versions },
  };
  const result = await (() => {
    switch (owner) {
      case "product":
        return client.PATCH("/admin/products/{id}/custom-fields", request);
      case "variant":
        return client.PATCH("/admin/variants/{id}/custom-fields", request);
      case "category":
        return client.PATCH("/admin/categories/{id}/custom-fields", request);
      case "customer":
        return client.PATCH("/admin/customers/{id}/custom-fields", request);
      case "order":
        return client.PATCH("/admin/orders/{id}/custom-fields", request);
    }
  })();
  const page = PAGES[owner](id);
  if (page) revalidatePath(page);
  if (result.error) {
    return {
      error: describe(result.error),
      problems: problemsOf(result.error.code, result.error.details),
    };
  }
  return { fields: result.data };
}

/** What a reference points at, for people: a title, a name, a path; or why it can't. */
export async function lookupReference(
  kind: string,
  id: string,
): Promise<{ label?: string; error?: string }> {
  const client = await sessionClient();
  const path = { params: { path: { id } } };
  const missing = { error: "Nothing with that ID." };
  switch (kind) {
    case "product": {
      const { data } = await client.GET("/admin/products/{id}", path);
      return data ? { label: data.title } : missing;
    }
    case "variant": {
      const { data } = await client.GET("/admin/variants/{id}/inventory", path);
      return data ? { label: "A variant of a product" } : missing;
    }
    case "category": {
      const { data } = await client.GET("/admin/categories/{id}", path);
      return data ? { label: data.name } : missing;
    }
    case "customer": {
      const { data } = await client.GET("/admin/customers/{id}", path);
      return data ? { label: customerName(data) || data.email } : missing;
    }
    case "asset": {
      const { data } = await client.GET("/admin/assets/{id}", path);
      return data ? { label: data.alt || data.filename } : missing;
    }
    default:
      return missing;
  }
}

/** Every category as a pickable path ("Clothing › Shirts"). */
export async function categoryOptions(): Promise<{ value: string; label: string }[]> {
  const client = await sessionClient();
  const { data } = await client.GET("/admin/categories");
  return flatten(data?.items ?? []).map((row) => ({ value: row.id, label: row.path }));
}
