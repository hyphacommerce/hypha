"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { deletePriceList } from "../actions";

/**
 * Deleting the list, for good. The API refuses while markets use it: then the card says
 * which, so they can be taken off first.
 */
export function DeletePriceList({ id, name }: { id: string; name: string }) {
  const [isAsking, setIsAsking] = useState(false);
  const [refusal, setRefusal] = useState<{ error: string; markets?: string[] }>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3} hAlign="start">
        <Text type="display-3" as="h2">
          Delete
        </Text>
        <Text type="supporting" color="secondary">
          Its fixed prices go with it. Orders placed with its prices keep them.
        </Text>
        {refusal && (
          <Banner
            status="error"
            title={refusal.error}
            description={
              refusal.markets && refusal.markets.length > 0
                ? `Used by ${refusal.markets.join(", ")}. Remove it there (under "Markets" on this page), then delete it.`
                : undefined
            }
          />
        )}
        <Button label="Delete price list" variant="destructive" onClick={() => setIsAsking(true)} />
      </VStack>
      <AlertDialog
        isOpen={isAsking}
        onOpenChange={(open) => !open && setIsAsking(false)}
        title={`Delete ${name}?`}
        description="The list and its fixed prices are deleted for good."
        actionLabel="Delete price list"
        isActionLoading={isPending}
        onAction={() =>
          startTransition(async () => {
            const result = await deletePriceList(id);
            setIsAsking(false);
            if (result.error) setRefusal({ error: result.error, markets: result.markets });
          })
        }
      />
    </Card>
  );
}
