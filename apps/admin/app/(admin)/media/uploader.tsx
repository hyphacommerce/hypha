"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { FileInput } from "@astryxdesign/core/FileInput";
import { VStack } from "@astryxdesign/core/Layout";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { completeUpload, startUpload } from "./actions";

/** What the API accepts (design note 0013); it checks again, by the file's contents. */
const ACCEPT = [
  "image/jpeg",
  "image/png",
  "image/webp",
  "image/avif",
  "image/gif",
  "video/mp4",
  "video/webm",
  "video/quicktime",
  "application/pdf",
].join(",");

/** The largest file of any kind (videos); the API enforces each kind's limit. */
const MAX_BYTES = 1024 * 1024 * 1024;

/**
 * Sends one file straight to storage with the request Hypha signed for it, then has
 * Hypha check it. The file never passes through the admin's server.
 */
async function upload(file: File): Promise<string | undefined> {
  const started = await startUpload({ name: file.name, type: file.type, size: file.size });
  if ("error" in started) return started.error;
  const headers = Object.fromEntries(started.upload.headers.map((h) => [h.name, h.value]));
  const stored = await fetch(started.upload.url, {
    method: started.upload.method,
    headers,
    body: file,
  }).catch(() => undefined);
  if (!stored?.ok) return "The upload didn't reach storage. Check your connection and retry.";
  return (await completeUpload(started.id)).error;
}

export function Uploader() {
  const router = useRouter();
  const [files, setFiles] = useState<File[]>([]);
  const [errors, setErrors] = useState<string[]>([]);
  const [isUploading, setIsUploading] = useState(false);

  const uploadAll = async (selected: File | File[] | null) => {
    const list = selected === null ? [] : Array.isArray(selected) ? selected : [selected];
    if (list.length === 0) return;
    setIsUploading(true);
    setErrors([]);
    const failed: string[] = [];
    // One at a time: a merchant dropping fifty photos shouldn't open fifty connections.
    for (const file of list) {
      const error = await upload(file);
      if (error) failed.push(`${file.name}: ${error}`);
    }
    setErrors(failed);
    setFiles([]);
    setIsUploading(false);
    router.refresh();
  };

  return (
    <VStack gap={2}>
      {errors.map((error) => (
        <Banner key={error} status="error" title={error} />
      ))}
      <FileInput
        label="Upload files"
        isLabelHidden
        mode="dropzone"
        isMultiple
        accept={ACCEPT}
        maxSize={MAX_BYTES}
        value={files}
        onChange={(selected) => setFiles(Array.isArray(selected) ? selected : [])}
        changeAction={uploadAll}
        isLoading={isUploading}
        placeholder="Drop images, videos or PDFs here, or choose files"
      />
    </VStack>
  );
}
