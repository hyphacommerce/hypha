"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { deleteDiscount } from "../actions";

/**
 * Deleting the discount, for good, with its codes and usage records. Orders keep what
 * it took off them; carts holding one of its codes see it go.
 */
export function DeleteDiscount({ id, title, used }: { id: string; title: string; used: number }) {
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
          Its codes and usage records go with it. Orders keep what it took off them. To stop it for
          now, switch it off under Schedule instead.
        </Text>
        {error && <Banner status="error" title={error} />}
        <Button label="Delete discount" variant="destructive" onClick={() => setIsAsking(true)} />
      </VStack>
      <AlertDialog
        isOpen={isAsking}
        onOpenChange={(open) => !open && setIsAsking(false)}
        title={`Delete ${title}?`}
        description={
          used > 0
            ? `It was used on ${used.toLocaleString("en")} ${used === 1 ? "order" : "orders"}: they keep their discount, but its usage records are deleted with it.`
            : "The discount and its codes are deleted for good."
        }
        actionLabel="Delete discount"
        isActionLoading={isPending}
        onAction={() =>
          startTransition(async () => {
            const result = await deleteDiscount(id);
            setIsAsking(false);
            if (result.refusal) setError(result.refusal.message);
          })
        }
      />
    </Card>
  );
}
