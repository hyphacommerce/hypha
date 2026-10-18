"use server";

import type { AdminSchemas } from "@hypha/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";
import { type Problem, problemOf } from "@/lib/notifications";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/** A template edit's outcome: also where in the sources it failed, when the API says. */
export type TemplateResult = Result & { problem?: Problem };

/** A JSON Merge Patch of the settings: absent keeps a field, `null` clears it. */
export type SettingsPatch = AdminSchemas["UpdateNotificationSettingsRequest"];

/** A template's sources as the editor holds them. */
export type Sources = { subject: string; html: string; text: string | null };

type Rendered = AdminSchemas["RenderedNotificationResponse"];

function refresh() {
  revalidatePath("/settings/notifications", "layout");
}

export async function updateNotificationSettings(patch: SettingsPatch): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/notification-settings", { body: patch });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** Turns a template on or off, keeping its sources. */
export async function setTemplateEnabled(key: string, enabled: boolean): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/notification-templates/{key}", {
    params: { path: { key } },
    body: { enabled },
  });
  if (error) return { error: describe(error) };
  refresh();
  return {};
}

/** The layout takes only HTML: its subject and text are left out. */
function body(key: string, sources: Sources) {
  return key === "layout"
    ? { html: sources.html }
    : { subject: sources.subject, html: sources.html, text: sources.text };
}

export async function saveTemplate(key: string, sources: Sources): Promise<TemplateResult> {
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/notification-templates/{key}", {
    params: { path: { key } },
    body: body(key, sources),
  });
  if (error) return { error: describe(error), problem: problemOf(error.details) };
  refresh();
  return {};
}

/** Goes back to the default; answers with its sources, for the editor to show. */
export async function revertTemplate(key: string): Promise<Result & { sources?: Sources }> {
  const client = await sessionClient();
  const { data, error } = await client.DELETE("/admin/notification-templates/{key}", {
    params: { path: { key } },
  });
  if (error) return { error: describe(error) };
  refresh();
  return { sources: { subject: data.subject, html: data.html, text: data.text ?? null } };
}

/** The unsaved sources rendered against the sample, or against `order` as it is now. */
export async function previewTemplate(
  key: string,
  sources: Sources,
  order: string | null,
): Promise<TemplateResult & { rendered?: Rendered }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/notification-templates/{key}/preview", {
    params: { path: { key } },
    body: { ...body(key, sources), order },
  });
  if (error) return { error: describe(error), problem: problemOf(error.details) };
  return { rendered: data };
}

/** Sends the unsaved sources, rendered, to the signed-in staff member's own address. */
export async function sendTestEmail(
  key: string,
  sources: Sources,
  order: string | null,
): Promise<TemplateResult & { sentTo?: string }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/notification-templates/{key}/test", {
    params: { path: { key } },
    body: { ...body(key, sources), order },
  });
  if (error) return { error: describe(error), problem: problemOf(error.details) };
  return { sentTo: data.sent_to };
}
