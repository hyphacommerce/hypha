"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { deleteSupplier } from "../actions";

/**
 * Deleting the supplier with its catalogue, for good. One that purchase orders name
 * can't be: it's archived instead, so the orders keep it.
 */
export function DeleteSupplier({ id, name }: { id: string; name: string }) {
  const t = useT<"suppliers">();
  const [isAsking, setIsAsking] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3} hAlign="start">
        <Text type="display-3" as="h2">
          {t("common.delete")}
        </Text>
        <Text type="supporting" color="secondary">
          {t("suppliers.delete_hint")}
        </Text>
        {error && <Banner status="error" title={error} />}
        <Button
          label={t("suppliers.delete_supplier")}
          variant="destructive"
          onClick={() => setIsAsking(true)}
        />
      </VStack>
      <AlertDialog
        isOpen={isAsking}
        onOpenChange={(open) => !open && setIsAsking(false)}
        title={t("common.delete_named", { name })}
        description={t("suppliers.delete_dialog_description")}
        actionLabel={t("suppliers.delete_dialog_action")}
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
