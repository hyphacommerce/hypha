"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
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
 * Uninstalls an app: its grant ends at once and it gets only `app.uninstalled`.
 * Removing an app that is already uninstalled, or one made by hand, deletes it, and
 * then the page it was on is gone.
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

/**
 * Narrows an installed app's grant to `scopes`: the others stop working at once.
 */
export async function narrowGrant(key: string, scopes: AdminSchemas["Scope"][]): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/apps/{key}/grant", {
    params: { path: { key } },
    body: { scopes },
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/apps/${key}`);
  return {};
}

/** `settings_invalid`'s `details.errors` (`[{path, message}]`) as messages by field. */
function fieldProblems(details: unknown): Record<string, string> {
  const fields: Record<string, string> = {};
  if (typeof details !== "object" || details === null || !("errors" in details)) return fields;
  const { errors } = details;
  if (!Array.isArray(errors)) return fields;
  for (const problem of errors) {
    if (typeof problem !== "object" || problem === null) continue;
    const path: unknown = "path" in problem ? problem.path : undefined;
    const message: unknown = "message" in problem ? problem.message : undefined;
    if (typeof path === "string" && path && typeof message === "string") fields[path] = message;
  }
  return fields;
}

/** What saving a spore's settings gave: the form's problems by field, if any. */
export type SettingsResult = Result & { fields?: Record<string, string> };

/**
 * Saves a built-in spore's settings. A secret field left out keeps what's stored; the API
 * seals it and never answers it.
 */
export async function saveSporeSettings(
  key: string,
  settings: Record<string, unknown>,
): Promise<SettingsResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/apps/{key}/settings", {
    params: { path: { key } },
    body: { settings },
  });
  if (error) {
    return { error: describe(error), fields: fieldProblems(error.details) };
  }
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
