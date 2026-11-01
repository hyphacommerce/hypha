"use client";

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { useState, useTransition } from "react";

import { type PurchaseOrder, poNumber } from "@/lib/purchasing";

import { deletePurchaseOrder, movePurchaseOrder } from "../actions";

type Confirm = "close" | "cancel" | "delete";

const CONFIRMS: Record<Confirm, { title: string; description: string; action: string }> = {
  close: {
    title: "Close this purchase order?",
    description:
      "What hasn't arrived is cancelled: nothing more is expected. Received goods stay received.",
    action: "Close",
  },
  cancel: {
    title: "Cancel this purchase order?",
    description: "Nothing arrived on it: the supplier owes nothing, and it can't be ordered again.",
    action: "Cancel order",
  },
  delete: {
    title: "Delete this draft?",
    description: "It was never ordered, so it's removed for good.",
    action: "Delete",
  },
};

/**
 * What can happen to a purchase order next, by its status: a draft is ordered, cancelled
 * or deleted; an ordered one closed or (with nothing received) cancelled.
 */
export function PurchaseOrderActions({ po }: { po: PurchaseOrder }) {
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
        <Button label="Delete" variant="ghost" onClick={() => setConfirm("delete")} />
        <Button label="Cancel" variant="secondary" onClick={() => setConfirm("cancel")} />
        <Button
          label="Mark as ordered"
          variant="primary"
          isLoading={isPending && !confirm}
          onClick={() => run(() => movePurchaseOrder(po.id, "order"))}
        />
      </>
    ) : po.status === "ordered" ? (
      <>
        {!arrived && (
          <Button label="Cancel" variant="secondary" onClick={() => setConfirm("cancel")} />
        )}
        <Button label="Close" variant="secondary" onClick={() => setConfirm("close")} />
      </>
    ) : null;
  if (!buttons) return null;
  const text = confirm && CONFIRMS[confirm];
  return (
    <VStack gap={2}>
      {error && <Banner status="error" title={error} />}
      <HStack gap={2} wrap="wrap">
        {buttons}
      </HStack>
      <AlertDialog
        isOpen={confirm !== undefined}
        onOpenChange={(open) => !open && setConfirm(undefined)}
        title={text ? text.title : ""}
        description={text ? `${poNumber(po.number)}: ${text.description}` : ""}
        actionLabel={text?.action ?? ""}
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
