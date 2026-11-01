"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { createPurchaseOrder } from "./actions";
import { purchaseOrderChoices } from "./choices";

type Choices = Awaited<ReturnType<typeof purchaseOrderChoices>>;

/**
 * Starts a draft: the supplier (active ones) and where the goods go. Lines, costs and
 * dates are added on its page. `supplier` preselects one (from a supplier's page).
 */
export function NewPurchaseOrder({
  supplier: preset,
  label = "New purchase order",
}: {
  supplier?: string;
  label?: string;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [choices, setChoices] = useState<Choices>();
  const [supplier, setSupplier] = useState(preset ?? "");
  const [destination, setDestination] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const open = () => {
    setIsOpen(true);
    startTransition(async () => {
      const found = await purchaseOrderChoices();
      setChoices(found);
      if (!destination && found.locations[0]) setDestination(found.locations[0].id);
    });
  };
  const close = () => {
    setIsOpen(false);
    setError(undefined);
  };
  const canSave = supplier !== "";
  const save = () => {
    if (!canSave) return;
    startTransition(async () => {
      const result = await createPurchaseOrder({
        supplier,
        destination: destination || null,
      });
      if (result?.error) setError(result.error);
    });
  };
  return (
    <>
      <Button label={label} variant="primary" onClick={open} />
      <Dialog isOpen={isOpen} onOpenChange={(o) => !o && close()} width={460} purpose="form">
        <DialogHeader
          title="New purchase order"
          subtitle="Lines, extra costs and dates come next, on its page."
          onOpenChange={(o) => !o && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          {choices && choices.suppliers.length === 0 ? (
            <Text type="supporting" color="secondary">
              Add a supplier first: purchase orders are placed with one.
            </Text>
          ) : (
            <Selector
              label="Supplier"
              options={(choices?.suppliers ?? []).map((s) => ({
                value: s.id,
                label: `${s.name} · ${s.currency}`,
              }))}
              value={supplier || undefined}
              onChange={setSupplier}
              placeholder={choices ? "Choose a supplier" : "Loading…"}
              hasSearch
            />
          )}
          <Selector
            label="Goes to"
            description="The location the goods are received at."
            options={(choices?.locations ?? []).map((l) => ({ value: l.id, label: l.name }))}
            value={destination || undefined}
            onChange={setDestination}
            placeholder={choices ? "Choose a location" : "Loading…"}
          />
          <HStack gap={2} hAlign="end">
            <Button label="Cancel" variant="ghost" onClick={close} />
            <Button
              label="Write draft"
              variant="primary"
              isLoading={isPending && !!choices}
              isDisabled={!canSave}
              onClick={save}
            />
          </HStack>
        </VStack>
      </Dialog>
    </>
  );
}
