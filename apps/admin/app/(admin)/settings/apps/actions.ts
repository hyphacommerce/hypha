"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };
/** A result with the app's signing secret, shown once. */
export type SecretResult = { error?: string; secret?: string };

const PAGE = "/settings/apps";

/** "order.*, product.created" → ["order.*", "product.created"]. The API checks each. */
function subscriptions(input: string): string[] {
  return input
    .split(/[\s,]+/)
    .map((pattern) => pattern.trim())
    .filter(Boolean);
}

/** The webhook as the form gives it: none without a URL. */
function webhook(url: string, timeoutMs: number | undefined) {
  return url.trim() ? { url: url.trim(), timeout_ms: timeoutMs } : null;
}

export async function createApp(input: {
  key: string;
  name: string;
  webhookUrl: string;
  timeoutMs?: number;
  subscriptions: string;
}): Promise<SecretResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/apps", {
    body: {
      key: input.key,
      name: input.name,
      webhook: webhook(input.webhookUrl, input.timeoutMs),
      subscriptions: subscriptions(input.subscriptions),
    },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return { secret: data.secret };
}

export async function updateApp(
  key: string,
  input: { name: string; webhookUrl: string; timeoutMs?: number; subscriptions: string },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/apps/{key}", {
    params: { path: { key } },
    body: {
      name: input.name,
      webhook: webhook(input.webhookUrl, input.timeoutMs),
      subscriptions: subscriptions(input.subscriptions),
    },
  });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  return {};
}

export async function setEnabled(key: string, enabled: boolean): Promise<Result> {
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

export async function deleteApp(key: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/apps/{key}", { params: { path: { key } } });
  if (error) return { error: describe(error) };
  revalidatePath(PAGE);
  revalidatePath("/apps");
  return {};
}
