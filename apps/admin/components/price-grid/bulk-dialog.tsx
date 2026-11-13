"use client";

// The grid's bulk actions that need a number: "Set to an amount" and "± % of base".

import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState } from "react";

import { parseAmount, parsePercent } from "@/lib/price-grid/money";
import { useT } from "@/lib/use-t";

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
  const t = useT<"price_lists">();
  const [text, setText] = useState("");
  const parsed =
    action === "amount" ? parseAmount(text, digits, currency, t) : parsePercent(text, t);
  const apply = () => {
    if (!parsed.ok) return;
    onApply(parsed.value);
    onClose();
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={420} purpose="form">
      <DialogHeader
        title={
          action === "amount"
            ? t("price_lists.bulk_amount_title")
            : t("price_lists.bulk_percent_title")
        }
        subtitle={
          action === "amount"
            ? t("price_lists.bulk_amount_subtitle", { count: cells, currency })
            : t("price_lists.bulk_percent_subtitle", { count: cells })
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        <TextInput
          label={
            action === "amount"
              ? t("price_lists.bulk_amount_label", { currency })
              : t("price_lists.bulk_percent_label")
          }
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
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.apply")}
            variant="primary"
            isDisabled={!parsed.ok}
            onClick={apply}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
