"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";
import { type Problem, problemOf, templateQuery } from "@/lib/notifications";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

/** A template edit's outcome: also where in the sources it failed, when the API says. */
export type TemplateResult = Result & { problem?: Problem };

/** A JSON Merge Patch of the settings: absent keeps a field, `null` clears it. */
export type SettingsPatch = AdminSchemas["UpdateNotificationSettingsRequest"];

/** A template's sources as the editor holds them. */
export type Sources = { subject: string; html: string; text: string | null };

/** Where a template's sources come from: whose, and in which language. */
export type SourcesFrom = { language: string; origin: string };

type Rendered = AdminSchemas["RenderedNotificationResponse"];

function refresh() {
  revalidatePath("/settings/notifications", "layout");
}

export async function updateNotificationSettings(patch: SettingsPatch): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/notification-settings", { body: patch });
  if (error) return { error: await describe(error) };
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
  if (error) return { error: await describe(error) };
  refresh();
  return {};
}

/** The layout takes only HTML: its subject and text are left out. */
function body(key: string, sources: Sources) {
  return key === "layout"
    ? { html: sources.html }
    : { subject: sources.subject, html: sources.html, text: sources.text };
}

/** Saves `sources` as the template in `language` (`null`: the store's content language). */
export async function saveTemplate(
  key: string,
  language: string | null,
  sources: Sources,
): Promise<TemplateResult & { from?: SourcesFrom }> {
  const client = await sessionClient();
  const { data, error } = await client.PUT("/admin/notification-templates/{key}", {
    params: { path: { key }, query: templateQuery(language) },
    body: body(key, sources),
  });
  if (error) return { error: await describe(error), problem: problemOf(error.details) };
  refresh();
  return { from: { language: data.sources_language, origin: data.sources_origin } };
}

/**
 * Deletes the merchant's template in `language`; answers with what emails in it send now,
 * and where that comes from, for the editor to show.
 */
export async function revertTemplate(
  key: string,
  language: string | null,
): Promise<Result & { sources?: Sources; from?: SourcesFrom }> {
  const client = await sessionClient();
  const { data, error } = await client.DELETE("/admin/notification-templates/{key}", {
    params: { path: { key }, query: templateQuery(language) },
  });
  if (error) return { error: await describe(error) };
  refresh();
  return {
    sources: { subject: data.subject, html: data.html, text: data.text ?? null },
    from: { language: data.sources_language, origin: data.sources_origin },
  };
}

/**
 * The unsaved sources rendered against the sample, or against `order` as it is now, in
 * `language`'s layout and formats.
 */
export async function previewTemplate(
  key: string,
  language: string | null,
  sources: Sources,
  order: string | null,
): Promise<TemplateResult & { rendered?: Rendered }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/notification-templates/{key}/preview", {
    params: { path: { key } },
    body: { ...body(key, sources), order, language },
  });
  if (error) return { error: await describe(error), problem: problemOf(error.details) };
  return { rendered: data };
}

/**
 * Sends the unsaved sources, rendered in `language`, to the signed-in staff member's own
 * address.
 */
export async function sendTestEmail(
  key: string,
  language: string | null,
  sources: Sources,
  order: string | null,
): Promise<TemplateResult & { sentTo?: string }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/notification-templates/{key}/test", {
    params: { path: { key } },
    body: { ...body(key, sources), order, language },
  });
  if (error) return { error: await describe(error), problem: problemOf(error.details) };
  return { sentTo: data.sent_to };
}
