"use server";

// Customer groups: the groups themselves and who's in them.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/** The groups page, the customers list (its filter) and the customer pages (their card). */
function refresh() {
  revalidatePath("/customers", "layout");
}

export async function createGroup(
  body: AdminSchemas["CreateCustomerGroupRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/customer-groups", { body });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

export async function updateGroup(
  id: string,
  body: AdminSchemas["UpdateCustomerGroupRequest"],
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/customer-groups/{id}", {
    params: { path: { id } },
    body,
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Deletes a group with its membership; its customers stay. */
export async function deleteGroup(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/customer-groups/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Makes `groups` the customer's groups, whole. */
export async function setCustomerGroups(customer: string, groups: string[]): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/customers/{id}/groups", {
    params: { path: { id: customer } },
    body: { groups },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/customers/${customer}`);
  revalidatePath("/customers");
  return {};
}
