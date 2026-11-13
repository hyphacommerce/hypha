"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { useState, useTransition } from "react";

import { type PurchaseOrder, poNumber } from "@/lib/purchasing";
import { useT } from "@/lib/use-t";

import { deletePurchaseOrder, movePurchaseOrder } from "../actions";

type Confirm = "close" | "cancel" | "delete";

/**
 * What can happen to a purchase order next, by its status: a draft is ordered, cancelled
 * or deleted; an ordered one closed or (with nothing received) cancelled.
 */
export function PurchaseOrderActions({ po }: { po: PurchaseOrder }) {
  const t = useT<"purchasing">();
  const [confirm, setConfirm] = useState<Confirm>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const run = (work: () => Promise<{ error?: string }>) =>
    startTransition(async () => {
      const result = await work();
      setConfirm(undefined);
      setError(result?.error);
    });
  const arrived = po.lines.some((l) => l.received + l.rejected > 0);
  const buttons =
    po.status === "draft" ? (
      <>
        <Button label={t("common.delete")} variant="ghost" onClick={() => setConfirm("delete")} />
        <Button
          label={t("purchasing.action_cancel")}
          variant="secondary"
          onClick={() => setConfirm("cancel")}
        />
        <Button
          label={t("purchasing.action_mark_ordered")}
          variant="primary"
          isLoading={isPending && !confirm}
          onClick={() => run(() => movePurchaseOrder(po.id, "order"))}
        />
      </>
    ) : po.status === "ordered" ? (
      <>
        {!arrived && (
          <Button
            label={t("purchasing.action_cancel")}
            variant="secondary"
            onClick={() => setConfirm("cancel")}
          />
        )}
        <Button
          label={t("purchasing.action_close")}
          variant="secondary"
          onClick={() => setConfirm("close")}
        />
      </>
    ) : null;
  if (!buttons) return null;
  const number = poNumber(po.number);
  const actions = {
    close: t("purchasing.action_close"),
    cancel: t("purchasing.confirm_cancel_action"),
    delete: t("common.delete"),
  };
  return (
    <VStack gap={2}>
      {error && <Banner status="error" title={error} />}
      <HStack gap={2} wrap="wrap">
        {buttons}
      </HStack>
      <AlertDialog
        isOpen={confirm !== undefined}
        onOpenChange={(open) => !open && setConfirm(undefined)}
        title={confirm ? t(`purchasing.confirm_${confirm}_title`) : ""}
        description={confirm ? t(`purchasing.confirm_${confirm}_description`, { number }) : ""}
        actionLabel={confirm ? actions[confirm] : ""}
        isActionLoading={isPending}
        onAction={() =>
          run(() =>
            confirm === "delete"
              ? deletePurchaseOrder(po.id)
              : movePurchaseOrder(po.id, confirm === "close" ? "close" : "cancel"),
          )
        }
      />
    </VStack>
  );
}
