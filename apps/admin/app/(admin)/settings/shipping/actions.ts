"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

const PAGE = "/settings/shipping";

/** "HR, si ,DE" → ["HR", "SI", "DE"]. The API checks each code. */
function countries(input: string): string[] {
  return input
    .split(/[\s,]+/)
    .map((code) => code.trim().toUpperCase())
    .filter(Boolean);
}

export async function createZone(input: { name: string; countries: string }): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/shipping-zones", {
    body: { name: input.name, countries: countries(input.countries) },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function updateZone(
  id: string,
  input: { name: string; countries: string },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/shipping-zones/{id}", {
    params: { path: { id } },
    body: { name: input.name, countries: countries(input.countries) },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function deleteZone(id: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/shipping-zones/{id}", {
    params: { path: { id } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export type OptionInput = {
  name: string;
  code: string;
  rate: AdminSchemas["RateDto"];
  estimate: AdminSchemas["DeliveryEstimateDto"] | null;
  is_backup: boolean;
};

export async function addOption(zone: string, input: OptionInput): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/shipping-zones/{id}/options", {
    params: { path: { id: zone } },
    body: input,
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function updateOption(
  zone: string,
  option: string,
  input: Partial<OptionInput> & { enabled?: boolean },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/shipping-zones/{id}/options/{option_id}", {
    params: { path: { id: zone, option_id: option } },
    body: input,
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function removeOption(zone: string, option: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/shipping-zones/{id}/options/{option_id}", {
    params: { path: { id: zone, option_id: option } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function reorderOptions(zone: string, options: string[]): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/shipping-zones/{id}/options/order", {
    params: { path: { id: zone } },
    body: { options },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

/** A result with the app's signing secret, shown once. */
export type SecretResult = { error?: string; secret?: string };

/**
 * Adds a rate app: the app (design note 0016), unless one with the key exists, then its
 * rate settings. The secret comes back only when the app is new.
 */
export async function createProvider(input: {
  key: string;
  name: string;
  callbackUrl: string;
  timeoutMs: number;
}): Promise<SecretResult> {
  const client = await sessionClient();
  const app = await client.POST("/admin/apps", { body: { key: input.key, name: input.name } });
  if (app.error && app.error.code !== "app_exists") return { error: describe(app.error) };
  const { error } = await client.POST("/admin/shipping-providers", {
    body: { key: input.key, callback_url: input.callbackUrl, timeout_ms: input.timeoutMs },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return { secret: app.data?.secret };
}

/** Turns the whole app on or off: its rates and its webhooks. */
export async function setAppEnabled(key: string, enabled: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/apps/{key}", {
    params: { path: { key } },
    body: { enabled },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function rotateSecret(key: string): Promise<SecretResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/apps/{key}/secret", {
    params: { path: { key } },
  });
  if (error) return { error: describe(error) };
  return { secret: data.secret };
}

/** Stops the app quoting rates; the app itself stays (Settings → Apps). */
export async function deleteProvider(key: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/shipping-providers/{key}", {
    params: { path: { key } },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

/** Asks the app for its services: whether its callback answers, signed, right now. */
export async function testProvider(
  key: string,
): Promise<{ error?: string; services?: AdminSchemas["ServiceDto"][] }> {
  const client = await sessionClient();
  const { data, error } = await client.GET("/admin/shipping-providers/{key}/services", {
    params: { path: { key } },
  });
  if (error) return { error: describe(error) };
  return { services: data.items };
}
