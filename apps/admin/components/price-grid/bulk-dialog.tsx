"use client";

// The grid's bulk actions that need a number: "Set to an amount" and "± % of base".

import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState } from "react";

import { parseAmount, parsePercent } from "@/lib/price-grid/money";

export type BulkAction = "amount" | "percent";

export function BulkDialog({
  action,
  currency,
  digits,
  cells,
  onApply,
  onClose,
}: {
  action: BulkAction;
  currency: string;
  digits: number;
  /** How many selected cells it changes. */
  cells: number;
  /** Minor units for "amount", basis points for "percent". */
  onApply: (value: number) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const parsed = action === "amount" ? parseAmount(text, digits) : parsePercent(text);
  const apply = () => {
    if (!parsed.ok) return;
    onApply(parsed.value);
    onClose();
  };
  const what = cells === 1 ? "the selected price" : `the ${cells} selected prices`;

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={420} purpose="form">
      <DialogHeader
        title={action === "amount" ? "Set to an amount" : "Set to a percentage of base"}
        subtitle={
          action === "amount"
            ? `Sets ${what} to one amount in ${currency}.`
            : `Sets ${what} to each variant's base price adjusted: -10 is 10% off, 5 is 5% more. Rounded as checkout rounds.`
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        <TextInput
          label={action === "amount" ? `Amount, ${currency}` : "Adjustment, %"}
          placeholder={action === "amount" ? "18.00" : "-10"}
          value={text}
          onChange={setText}
          onEnter={apply}
          hasAutoFocus
          status={
            text.trim() !== "" && !parsed.ok ? { type: "error", message: parsed.error } : undefined
          }
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Apply" variant="primary" isDisabled={!parsed.ok} onClick={apply} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
