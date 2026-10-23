"use server";

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";

export type FormState = { error?: string; saved?: boolean };

/** The signed request the browser sends the file with, straight to storage. */
export type Upload = AdminSchemas["UploadRequestResponse"];

/**
 * Starts an upload: a pending asset, and the request that sends the file to storage.
 * The browser sends it (the file never passes through the admin), then calls
 * `completeUpload`. The signed URL only allows this one file, for 15 minutes.
 */
export async function startUpload(file: {
  name: string;
  type: string;
  size: number;
}): Promise<{ id: string; upload: Upload } | { error: string }> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/assets", {
    body: { filename: file.name, mime_type: file.type, byte_size: file.size },
  });
  if (error) return { error: describe(error) };
  return { id: data.asset.id, upload: data.upload };
}

/** Has Hypha check the stored file and make the asset ready. */
export async function completeUpload(id: string): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/assets/{id}/complete", {
    params: { path: { id } },
  });
  revalidatePath("/media");
  if (error) return { error: describe(error) };
  return {};
}

export async function updateAsset(
  _previous: FormState,
  input: { id: string; alt?: string; title: string },
): Promise<FormState> {
  const client = await sessionClient();
  // Blank clears (the API trims). Without `alt` (the form only edits it for images) the
  // stored alt text stays as it is: Merge Patch leaves absent fields alone.
  const body =
    input.alt === undefined ? { title: input.title } : { alt: input.alt, title: input.title };
  const { error } = await client.PATCH("/admin/assets/{id}", {
    params: { path: { id: input.id } },
    body,
  });
  if (error) return { error: describe(error) };
  revalidatePath(`/media/${input.id}`);
  revalidatePath("/media");
  return { saved: true };
}

export async function deleteAsset(id: string): Promise<FormState> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/assets/{id}", { params: { path: { id } } });
  if (error) return { error: describe(error) };
  revalidatePath("/media");
  redirect("/media");
}
