"use client";

import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { deleteAsset } from "../actions";

/** Delete, asked twice in place (no browser dialog): the file goes for good. */
export function DeleteAsset({ id }: { id: string }) {
  const [isConfirming, setIsConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  if (!isConfirming) {
    return <Button label="Delete" variant="ghost" onClick={() => setIsConfirming(true)} />;
  }
  return (
    <HStack gap={2} vAlign="center">
      <Text type="supporting">{error ?? "Delete this file for good?"}</Text>
      <Button label="Cancel" variant="ghost" onClick={() => setIsConfirming(false)} />
      <Button
        label="Delete file"
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
