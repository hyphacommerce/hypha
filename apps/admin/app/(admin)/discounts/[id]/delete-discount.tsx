"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { deleteDiscount } from "../actions";

/**
 * Deleting the discount, for good, with its codes and usage records. Orders keep what
 * it took off them; carts holding one of its codes see it go.
 */
export function DeleteDiscount({ id, title, used }: { id: string; title: string; used: number }) {
  const t = useT<"discounts">();
  const [isAsking, setIsAsking] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3} hAlign="start">
        <Text type="display-3" as="h2">
          {t("discounts.delete_title")}
        </Text>
        <Text type="supporting" color="secondary">
          {t("discounts.delete_hint")}
        </Text>
        {error && <Banner status="error" title={error} />}
        <Button
          label={t("discounts.delete_action")}
          variant="destructive"
          onClick={() => setIsAsking(true)}
        />
      </VStack>
      <AlertDialog
        isOpen={isAsking}
        onOpenChange={(open) => !open && setIsAsking(false)}
        title={t("common.delete_named", { name: title })}
        description={
          used > 0
            ? t("discounts.delete_dialog_used", { count: used })
            : t("discounts.delete_dialog_unused")
        }
        actionLabel={t("discounts.delete_dialog_action")}
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
