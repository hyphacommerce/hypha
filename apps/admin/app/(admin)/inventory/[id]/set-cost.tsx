"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { parseUnitCost, unitCostInput } from "@/lib/format";

import { setUnitCost } from "../actions";

/**
 * Sets what one unit cost by hand, in the store's base currency, for stock bought
 * outside purchase orders. Blank clears it. The history keeps the cost before and after.
 */
export function SetCost({
  item,
  cost,
  currency,
}: {
  item: string;
  cost: AdminSchemas["UnitCost"] | null;
  /** The store's base currency. */
  currency: string;
}) {
  const currencies = useCurrencies();
  const initial = cost ? unitCostInput(cost, currencies) : "";
  const [isOpen, setIsOpen] = useState(false);
  const [amount, setAmount] = useState(initial);
  const [note, setNote] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const parsed = amount.trim() === "" ? null : parseUnitCost(amount, currency, currencies);
  const isValid = amount.trim() === "" || parsed !== null;
  const close = () => {
    setIsOpen(false);
    setAmount(initial);
    setNote("");
    setError(undefined);
  };
  const save = () => {
    if (!isValid) return;
    startTransition(async () => {
      const result = await setUnitCost(item, {
        cost: parsed === null ? null : { amount: parsed, currency },
        note,
      });
      if (result.error) setError(result.error);
      else close();
    });
  };

  return (
    <>
      <Button
        label={cost ? "Change cost" : "Set cost"}
        variant="secondary"
        onClick={() => setIsOpen(true)}
      />
      <Dialog isOpen={isOpen} onOpenChange={(open) => !open && close()} width={440} purpose="form">
        <DialogHeader
          title="Unit cost"
          subtitle="What one unit cost you. Receiving a purchase order updates it as goods arrive."
          onOpenChange={(open) => !open && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          <TextInput
            label={`Cost per unit (${currency})`}
            description="Up to 4 decimals past the cent: 0.0035 for a part bought by the thousand. Blank clears it."
            value={amount}
            onChange={setAmount}
            status={isValid ? undefined : { type: "error", message: "An amount like 4.125" }}
            width={200}
          />
          <TextArea
            label="Note"
            isOptional
            maxLength={500}
            rows={2}
            value={note}
            onChange={setNote}
          />
          <HStack gap={2} hAlign="end">
            <Button label="Cancel" variant="ghost" onClick={close} />
            <Button
              label="Save"
              variant="primary"
              isLoading={isPending}
              isDisabled={!isValid}
              onClick={save}
            />
          </HStack>
        </VStack>
      </Dialog>
    </>
  );
}
