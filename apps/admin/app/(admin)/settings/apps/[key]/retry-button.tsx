"use client";

import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { retryDelivery } from "../actions";

/** Sends a delivery again on the dispatcher's next run (within seconds). */
export function RetryButton({ appKey, id }: { appKey: string; id: string }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <HStack gap={2} hAlign="end" vAlign="center">
      {error && (
        <Text type="supporting" color="secondary">
          Couldn't retry: {error}
        </Text>
      )}
      <Button
        label="Retry"
        size="sm"
        variant="secondary"
        isLoading={isPending}
        onClick={() =>
          startTransition(async () => setError((await retryDelivery(appKey, id)).error))
        }
      />
    </HStack>
  );
}
