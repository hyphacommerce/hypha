"use server";

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an action: a message to show, if it failed. */
export type Result = { error?: string };

/** Sends a delivery again on the dispatcher's next run. */
export async function retryDelivery(key: string, id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/apps/{key}/deliveries/{id}/retry", {
    params: { path: { key, id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/apps/${key}/deliveries`);
  return {};
}

/**
 * Uninstalls an app (design note 0029): its grant ends at once and it gets only
 * `app.uninstalled`. Removing an app that is already uninstalled, or one made by hand,
 * deletes it, and then the page it was on is gone.
 */
export async function removeApp(key: string, isDelete: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/apps/{key}", { params: { path: { key } } });
  if (error) return { error: describe(error) };
  revalidatePath("/apps");
  if (isDelete) redirect("/apps");
  revalidatePath(`/apps/${key}`);
  return {};
}

/** Turns an app's webhooks and rate quotes on or off. */
export async function setAppEnabled(key: string, enabled: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/apps/{key}", {
    params: { path: { key } },
    body: { enabled },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/apps/${key}`);
  revalidatePath("/apps");
  return {};
}
