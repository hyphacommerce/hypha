// Uploading from the browser, shared by the Media page and the product page's Media
// section. Client code: it sends the file itself; the server actions only sign and check.

import { completeUpload, startUpload } from "./actions";

const IMAGE_TYPES = ["image/jpeg", "image/png", "image/webp", "image/avif", "image/gif"];
const VIDEO_TYPES = ["video/mp4", "video/webm", "video/quicktime"];

/** What the API accepts; it checks again, by the file's contents. */
export const ACCEPT_ANY = [...IMAGE_TYPES, ...VIDEO_TYPES, "application/pdf"].join(",");

/** What a product's media list takes: images and videos. */
export const ACCEPT_PRODUCT_MEDIA = [...IMAGE_TYPES, ...VIDEO_TYPES].join(",");

/** The largest file of any kind (videos); the API enforces each kind's limit. */
export const MAX_BYTES = 1024 * 1024 * 1024;

/**
 * Sends one file straight to storage with the request Hypha signed for it, then has
 * Hypha check it. The file never passes through the admin's server. The new asset's ID,
 * or a message for the person at the screen.
 */
export async function uploadFile(file: File): Promise<{ id: string } | { error: string }> {
  const started = await startUpload({ name: file.name, type: file.type, size: file.size });
  if ("error" in started) return started;
  const headers = Object.fromEntries(started.upload.headers.map((h) => [h.name, h.value]));
  const stored = await fetch(started.upload.url, {
    method: started.upload.method,
    headers,
    body: file,
  }).catch(() => undefined);
  if (!stored?.ok)
    return { error: "The upload didn't reach storage. Check your connection and retry." };
  const completed = await completeUpload(started.id);
  return completed.error ? { error: completed.error } : { id: started.id };
}
