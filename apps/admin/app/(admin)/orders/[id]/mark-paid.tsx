"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { useState, useTransition } from "react";

import { markOrderPaid } from "../actions";

export function MarkPaid({ id }: { id: string }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <VStack gap={2}>
      <Button
        label="Mark as paid"
        variant="primary"
        isLoading={isPending}
        onClick={() =>
          startTransition(async () => {
            setError((await markOrderPaid(id)).error);
          })
        }
      />
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}
