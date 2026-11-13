"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { markOrderPaid } from "../actions";

/** "Mark as paid", asking for what staff have to show for it (optional). */
export function MarkPaid({
  id,
  number,
  hasPayment,
  variant = "primary",
}: {
  id: string;
  number: number;
  /** Whether the order has an open payment to mark; otherwise a manual one is recorded. */
  hasPayment: boolean;
  variant?: "primary" | "secondary";
}) {
  const t = useT<"orders">();
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button
        label={t("orders.mark_paid_button")}
        variant={variant}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && (
        <MarkPaidDialog
          id={id}
          number={number}
          hasPayment={hasPayment}
          onClose={() => setIsOpen(false)}
        />
      )}
    </>
  );
}

function MarkPaidDialog({
  id,
  number,
  hasPayment,
  onClose,
}: {
  id: string;
  number: number;
  hasPayment: boolean;
  onClose: () => void;
}) {
  const t = useT<"orders">();
  const [reference, setReference] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const submit = () =>
    startTransition(async () => {
      const result = await markOrderPaid(id, reference);
      if (result.error) setError(result.error);
      else onClose();
    });
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        // An order's number is shown as it is, never grouped like a count.
        title={t("orders.mark_paid_dialog_title", { number: String(number) })}
        subtitle={
          hasPayment ? t("orders.mark_paid_dialog_collected") : t("orders.mark_paid_dialog_manual")
        }
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("orders.reference")}
          description={t("orders.mark_paid_reference_description")}
          isOptional
          value={reference}
          onChange={setReference}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("orders.mark_paid_button")}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
