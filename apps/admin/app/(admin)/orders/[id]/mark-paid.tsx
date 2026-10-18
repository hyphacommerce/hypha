"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

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
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button label="Mark as paid" variant={variant} onClick={() => setIsOpen(true)} />
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
        title={`Mark order #${number} as paid?`}
        subtitle={
          hasPayment
            ? "For money you collected yourself: the courier's cash, a bank transfer."
            : "It has no payment yet, so a manual payment of the total is recorded."
        }
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label="Reference"
          description="What you have to show for it: a bank reference, the courier's receipt."
          isOptional
          value={reference}
          onChange={setReference}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Mark as paid" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
