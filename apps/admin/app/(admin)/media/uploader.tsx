"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { FileInput } from "@astryxdesign/core/FileInput";
import { VStack } from "@astryxdesign/core/Layout";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { useT } from "@/lib/use-t";

import { ACCEPT_ANY, MAX_BYTES, uploadFile } from "./upload";

export function Uploader() {
  const t = useT<"media">();
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
    try {
      // One at a time: a merchant dropping fifty photos shouldn't open fifty connections.
      for (const file of list) {
        const uploaded = await uploadFile(file, t);
        if ("error" in uploaded) {
          failed.push(t("media.upload_file_error", { file: file.name, error: uploaded.error }));
        }
      }
    } finally {
      setErrors(failed);
      setFiles([]);
      setIsUploading(false);
      router.refresh();
    }
  };

  return (
    <VStack gap={2}>
      {errors.map((error) => (
        <Banner key={error} status="error" title={error} />
      ))}
      <FileInput
        label={t("media.upload_label")}
        isLabelHidden
        mode="dropzone"
        isMultiple
        accept={ACCEPT_ANY}
        maxSize={MAX_BYTES}
        value={files}
        onChange={(selected) => setFiles(Array.isArray(selected) ? selected : [])}
        changeAction={uploadAll}
        isLoading={isUploading}
        placeholder={t("media.upload_placeholder")}
      />
    </VStack>
  );
}
