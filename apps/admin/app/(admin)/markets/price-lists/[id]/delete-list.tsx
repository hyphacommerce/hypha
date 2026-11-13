"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { deletePriceList } from "../actions";

/**
 * Deleting the list, for good. The API refuses while markets use it: then the card says
 * which, so they can be taken off first.
 */
export function DeletePriceList({ id, name }: { id: string; name: string }) {
  const t = useT<"price_lists">();
  const format = useFormat();
  const [isAsking, setIsAsking] = useState(false);
  const [refusal, setRefusal] = useState<{ error: string; markets?: string[] }>();
  const [isPending, startTransition] = useTransition();
  return (
    <Card>
      <VStack gap={3} hAlign="start">
        <Text type="display-3" as="h2">
          {t("common.delete")}
        </Text>
        <Text type="supporting" color="secondary">
          {t("price_lists.delete_description")}
        </Text>
        {refusal && (
          <Banner
            status="error"
            title={refusal.error}
            description={
              refusal.markets && refusal.markets.length > 0
                ? t("price_lists.delete_in_use", { markets: format.list(refusal.markets, "unit") })
                : undefined
            }
          />
        )}
        <Button
          label={t("price_lists.delete_action")}
          variant="destructive"
          onClick={() => setIsAsking(true)}
        />
      </VStack>
      <AlertDialog
        isOpen={isAsking}
        onOpenChange={(open) => !open && setIsAsking(false)}
        title={t("common.delete_named", { name })}
        description={t("price_lists.delete_confirm_description")}
        actionLabel={t("price_lists.delete_action")}
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
