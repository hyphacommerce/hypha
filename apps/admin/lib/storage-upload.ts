// Sending a file from the browser straight to storage, with the request Hypha signed
// for it (an asset's or an upload's). The file never passes through the admin's server.

/** The request Hypha signed: `upload` in `POST /admin/assets` and `POST /admin/uploads`. */
export type SignedUpload = {
  method: string;
  url: string;
  headers: readonly { name: string; value: string }[];
};

/** Whether storage took `file`; false when it refused or couldn't be reached. */
export async function sendToStorage(upload: SignedUpload, file: Blob): Promise<boolean> {
  const headers = Object.fromEntries(upload.headers.map((h) => [h.name, h.value]));
  const stored = await fetch(upload.url, { method: upload.method, headers, body: file }).catch(
    () => undefined,
  );
  return Boolean(stored?.ok);
}
