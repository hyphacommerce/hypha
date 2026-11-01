"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { deleteSupplier } from "../actions";

/**
 * Deleting the supplier with its catalogue, for good. One that purchase orders name
 * can't be: it's archived instead, so the orders keep it.
 */
export function DeleteSupplier({ id, name }: { id: string; name: string }) {
  const [isAsking, setIsAsking] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3} hAlign="start">
        <Text type="display-3" as="h2">
          Delete
        </Text>
        <Text type="supporting" color="secondary">
          Its catalogue goes with it. A supplier on purchase orders can only be archived.
        </Text>
        {error && <Banner status="error" title={error} />}
        <Button label="Delete supplier" variant="destructive" onClick={() => setIsAsking(true)} />
      </VStack>
      <AlertDialog
        isOpen={isAsking}
        onOpenChange={(open) => !open && setIsAsking(false)}
        title={`Delete ${name}?`}
        description="The supplier and the prices they gave are deleted for good."
        actionLabel="Delete supplier"
        isActionLoading={isPending}
        onAction={() =>
          startTransition(async () => {
            const result = await deleteSupplier(id);
            setIsAsking(false);
            if (result?.error) setError(result.error);
          })
        }
      />
    </Card>
  );
}
