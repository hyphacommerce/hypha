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
  if (error) return { error: describe(error) };
  refresh();
  return {};
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
