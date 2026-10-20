"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

export type CreateRequest = AdminSchemas["CreateCustomFieldDefinitionRequest"];
export type UpdateRequest = AdminSchemas["UpdateCustomFieldDefinitionRequest"];

/** Definitions shape every owner's page and list filters: refresh them all. */
function refresh() {
  revalidatePath("/", "layout");
}

export async function createDefinition(body: CreateRequest): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/custom-field-definitions", { body });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

export async function updateDefinition(id: string, body: UpdateRequest): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/custom-field-definitions/{id}", {
    params: { path: { id } },
    body,
  });
  if (error) {
    if (error.code !== "custom_field_values_invalid") return { error: describe(error) };
    // Name the records to fix, not just how many (the first few of the API's list).
    const details: unknown = error.details;
    const owners: unknown =
      typeof details === "object" && details !== null ? Reflect.get(details, "owners") : undefined;
    const names = Array.isArray(owners) ? await ownerNames(client, owners.slice(0, 5)) : [];
    return {
      error: names.length
        ? `${describe(error)} Starting with: ${names.join(", ")}.`
        : describe(error),
    };
  }
  refresh();
  return {};
}

type Client = Awaited<ReturnType<typeof sessionClient>>;

/** What staff call each record (a product's title, a customer's email, …); its ID if it
 *  can't be read. */
async function ownerNames(client: Client, owners: unknown[]): Promise<string[]> {
  return Promise.all(
    owners.map(async (owner) => {
      const id = String(owner);
      const path = { params: { path: { id } } };
      if (id.startsWith("prod_")) {
        const { data } = await client.GET("/admin/products/{id}", path);
        return data?.title ?? id;
      }
      if (id.startsWith("pcat_")) {
        const { data } = await client.GET("/admin/categories/{id}", path);
        return data?.name ?? id;
      }
      if (id.startsWith("cus_")) {
        const { data } = await client.GET("/admin/customers/{id}", path);
        return data?.email ?? id;
      }
      if (id.startsWith("ord_")) {
        const { data } = await client.GET("/admin/orders/{id}", path);
        return data ? `order #${data.number}` : id;
      }
      return id;
    }),
  );
}

/** Moves a definition to `position` on its owner type's form; the others close up. */
export async function moveDefinition(id: string, position: number): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/custom-field-definitions/{id}/position", {
    params: { path: { id } },
    body: { position },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Deletes a definition with every value of it. */
export async function deleteDefinition(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/custom-field-definitions/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}
