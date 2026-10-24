"use client";

// The breaks cell's editor: up to ten "from this quantity, this price" rows for one
// variant, checked as they're typed against the fixed price the grid holds for it.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { TrashIcon } from "@heroicons/react/24/outline";
import { useState } from "react";

import {
  type BreakAmount,
  breakProblems,
  formatBreaks,
  parseBreaks,
} from "@/lib/price-grid/breaks";
import { formatAmount, parseAmount } from "@/lib/price-grid/money";
import { MAX_BREAKS } from "@/lib/price-grid/types";

type Row = { key: number; quantity: string; price: string };

function rowsOf(text: string, digits: number): Row[] {
  const parsed = parseBreaks(text, digits);
  if (!parsed.ok) return [];
  return parsed.value.map((b, key) => ({
    key,
    quantity: String(b.min_quantity),
    price: formatAmount(b.amount, digits),
  }));
}

export function BreaksDialog({
  title,
  currency,
  digits,
  fixedPrice,
  value,
  onSave,
  onClose,
}: {
  /** The variant's name, for the header. */
  title: string;
  currency: string;
  digits: number;
  /** The fixed price cell's text, as the grid holds it. */
  fixedPrice: string;
  /** The breaks cell's text ("10=8.00; 20=7.50"). */
  value: string;
  onSave: (text: string) => void;
  onClose: () => void;
}) {
  const [rows, setRows] = useState(() => rowsOf(value, digits));

  const fixed = parseAmount(fixedPrice, digits);
  const fixedAmount = fixed.ok ? fixed.value : null;
  const parsed = rows.map((row) => ({
    quantity: /^\d+$/.test(row.quantity.trim()) ? Number(row.quantity.trim()) : null,
    price: parseAmount(row.price, digits),
  }));
  const complete: BreakAmount[] = parsed.flatMap((p) =>
    p.quantity !== null && p.price.ok ? [{ min_quantity: p.quantity, amount: p.price.value }] : [],
  );
  const rowsValid = complete.length === rows.length;
  const problems = rowsValid ? breakProblems(complete, fixedAmount) : [];
  const canSave = rowsValid && problems.length === 0;

  const update = (key: number, field: "quantity" | "price", text: string) =>
    setRows((current) => current.map((r) => (r.key === key ? { ...r, [field]: text } : r)));

  // A key no row has, so React keeps each row's inputs with it as rows go.
  const add = () =>
    setRows((current) => [
      ...current,
      { key: Math.max(-1, ...current.map((r) => r.key)) + 1, quantity: "", price: "" },
    ]);

  const save = () => {
    if (!canSave) return;
    onSave(formatBreaks(complete, digits));
    onClose();
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={`Quantity breaks for ${title}`}
        subtitle={
          fixedAmount === null
            ? "Breaks need a fixed price: set one in the grid first."
            : `Below the first break, each costs the fixed price, ${formatAmount(fixedAmount, digits)} ${currency}.`
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={3} padding={4}>
        {problems.length > 0 && <Banner status="error" title={problems.join(" ")} />}
        {rows.length === 0 && (
          <Text color="secondary">No breaks: every quantity costs the same.</Text>
        )}
        {rows.map((row, index) => {
          const p = parsed[index];
          return (
            <HStack key={row.key} gap={2} vAlign="end">
              <TextInput
                label="From quantity"
                isLabelHidden={index > 0}
                placeholder="10"
                value={row.quantity}
                onChange={(text) => update(row.key, "quantity", text)}
                width={140}
                statusVariant="tooltip"
                status={
                  row.quantity.trim() !== "" && p?.quantity === null
                    ? { type: "error", message: "A whole number." }
                    : undefined
                }
              />
              <TextInput
                label={`Price each, ${currency}`}
                isLabelHidden={index > 0}
                placeholder="8.00"
                value={row.price}
                onChange={(text) => update(row.key, "price", text)}
                onEnter={save}
                statusVariant="tooltip"
                status={
                  row.price.trim() !== "" && p && !p.price.ok
                    ? { type: "error", message: p.price.error }
                    : undefined
                }
              />
              <IconButton
                label="Remove the break"
                icon={<TrashIcon />}
                variant="ghost"
                onClick={() => setRows((current) => current.filter((r) => r.key !== row.key))}
              />
            </HStack>
          );
        })}
        <HStack gap={2} hAlign="between">
          <Button
            label="Add break"
            variant="secondary"
            isDisabled={rows.length >= MAX_BREAKS}
            onClick={add}
          />
          <HStack gap={2}>
            <Button label="Cancel" variant="ghost" onClick={onClose} />
            <Button label="Done" variant="primary" isDisabled={!canSave} onClick={save} />
          </HStack>
        </HStack>
      </VStack>
    </Dialog>
  );
}
