"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { createPurchaseOrder } from "./actions";
import { purchaseOrderChoices } from "./choices";

type Choices = Awaited<ReturnType<typeof purchaseOrderChoices>>;

/**
 * Starts a draft: the supplier (active ones) and where the goods go. Lines, costs and
 * dates are added on its page. `supplier` preselects one (from a supplier's page).
 */
export function NewPurchaseOrder({
  supplier: preset,
  label,
}: {
  supplier?: string;
  label?: string;
}) {
  const t = useT<"purchasing">();
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
      <Button label={label ?? t("purchasing.new_title")} variant="primary" onClick={open} />
      <Dialog isOpen={isOpen} onOpenChange={(o) => !o && close()} width={460} purpose="form">
        <DialogHeader
          title={t("purchasing.new_title")}
          subtitle={t("purchasing.new_subtitle")}
          onOpenChange={(o) => !o && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          {choices && choices.suppliers.length === 0 ? (
            <Text type="supporting" color="secondary">
              {t("purchasing.new_no_suppliers")}
            </Text>
          ) : (
            <Selector
              label={t("common.supplier")}
              options={(choices?.suppliers ?? []).map((s) => ({
                value: s.id,
                label: `${s.name} · ${s.currency}`,
              }))}
              value={supplier || undefined}
              onChange={setSupplier}
              placeholder={choices ? t("purchasing.choose_supplier") : t("common.loading")}
              hasSearch
            />
          )}
          <Selector
            label={t("purchasing.goes_to")}
            description={t("purchasing.goes_to_description")}
            options={(choices?.locations ?? []).map((l) => ({ value: l.id, label: l.name }))}
            value={destination || undefined}
            onChange={setDestination}
            placeholder={choices ? t("purchasing.choose_location") : t("common.loading")}
          />
          <HStack gap={2} hAlign="end">
            <Button label={t("common.cancel")} variant="ghost" onClick={close} />
            <Button
              label={t("purchasing.write_draft")}
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
