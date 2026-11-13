"use client";

import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { deleteAsset } from "../actions";

/** Delete, asked twice in place (no browser dialog): the file goes for good. */
export function DeleteAsset({ id }: { id: string }) {
  const t = useT<"media">();
  const [isConfirming, setIsConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  if (!isConfirming) {
    return (
      <Button label={t("common.delete")} variant="ghost" onClick={() => setIsConfirming(true)} />
    );
  }
  return (
    <HStack gap={2} vAlign="center">
      <Text type="supporting">{error ?? t("media.delete_confirm")}</Text>
      <Button label={t("common.cancel")} variant="ghost" onClick={() => setIsConfirming(false)} />
      <Button
        label={t("media.delete_action")}
        variant="destructive"
        isLoading={isPending}
        onClick={() =>
          startTransition(async () => {
            const result = await deleteAsset(id);
            if (result?.error) setError(result.error);
          })
        }
      />
    </HStack>
  );
}
