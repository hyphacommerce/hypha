"use server";

import { revalidatePath } from "next/cache";

import { describe, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import {
  type FieldProblem,
  type PutTranslation,
  isKind,
  problemsOf,
  translateHref,
} from "@/lib/translations";

/** The outcome of a save: a message if it failed, with each refused field's problem. */
export type SaveResult = { error?: string; problems?: Record<string, FieldProblem> };

/**
 * Saves one language's translation of a resource: `body` is every field of that
 * language, since the API replaces them. No fields at all removes the translation.
 */
export async function saveTranslation(
  kind: string,
  id: string,
  language: string,
  body: PutTranslation,
): Promise<SaveResult> {
  if (!isKind(kind)) return { error: (await getT())("common.something_went_wrong") };
  const client = await sessionClient();
  const { error } = await client.PUT("/admin/translations/{kind}/{id}/{language}", {
    params: { path: { kind, id, language } },
    body,
  });
  if (error) {
    // A refused field often means the source moved (`source_changed`, a removed
    // option): the page reads it again, and the draft stays for another try.
    if (error.code === "invalid_translation") revalidatePath(translateHref(kind, id));
    return { error: await describe(error), problems: problemsOf(error.details) };
  }
  revalidatePath(translateHref(kind, id));
  return {};
}

/** Removes a language's whole translation of a resource: its source shows again. */
export async function removeTranslation(
  kind: string,
  id: string,
  language: string,
): Promise<{ error?: string }> {
  if (!isKind(kind)) return { error: (await getT())("common.something_went_wrong") };
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/translations/{kind}/{id}/{language}", {
    params: { path: { kind, id, language } },
  });
  if (error) return { error: await describe(error) };
  revalidatePath(translateHref(kind, id));
  return {};
}
