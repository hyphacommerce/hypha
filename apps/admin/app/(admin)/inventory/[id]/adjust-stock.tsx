"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Selector } from "@astryxdesign/core/Selector";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useState, useTransition } from "react";

import { ADJUSTMENT_REASONS, type AdjustmentReason, reasonLabel } from "@/lib/inventory";
import { useT } from "@/lib/use-t";

import { adjustStock } from "../actions";

/**
 * Adds or removes units at one location for a reason (damaged, lost, a sample…), kept
 * in the history with a note. Setting a count after counting the shelf is the product's
 * stock dialog.
 */
export function AdjustStock({
  item,
  locations,
}: {
  item: string;
  /** Locations by ID, in the order to offer them. */
  locations: [string, string][];
}) {
  const t = useT<"inventory">();
  const [isOpen, setIsOpen] = useState(false);
  const [location, setLocation] = useState(locations[0]?.[0]);
  const [delta, setDelta] = useState<number>();
  const [reason, setReason] = useState<AdjustmentReason>("correction");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const close = () => {
    setIsOpen(false);
    setDelta(undefined);
    setNote("");
    setError(undefined);
  };
  const canSave = location != null && delta != null && Number.isInteger(delta) && delta !== 0;
  const save = () => {
    if (!canSave) return;
    startTransition(async () => {
      const result = await adjustStock(item, { location, delta, reason, note });
      if (result.error) setError(result.error);
      else close();
    });
  };

  return (
    <>
      <Button
        label={t("inventory.adjust_button")}
        variant="secondary"
        isDisabled={locations.length === 0}
        onClick={() => setIsOpen(true)}
      />
      <Dialog isOpen={isOpen} onOpenChange={(open) => !open && close()} width={480} purpose="form">
        <DialogHeader
          title={t("inventory.adjust_title")}
          subtitle={t("inventory.adjust_subtitle")}
          onOpenChange={(open) => !open && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          <Selector
            label={t("common.location")}
            options={locations.map(([value, label]) => ({ value, label }))}
            value={location}
            onChange={setLocation}
          />
          <NumberInput
            label={t("inventory.adjust_change")}
            description={t("inventory.adjust_change_description")}
            value={delta}
            onChange={(value) => setDelta(value ?? undefined)}
            step={1}
            width={160}
          />
          <Selector
            label={t("inventory.adjust_reason")}
            options={ADJUSTMENT_REASONS.map((value) => ({ value, label: reasonLabel(t, value) }))}
            value={reason}
            onChange={(value) => {
              const chosen = ADJUSTMENT_REASONS.find((r) => r === value);
              if (chosen) setReason(chosen);
            }}
          />
          <TextArea
            label={t("common.note")}
            isOptional
            maxLength={500}
            rows={2}
            value={note}
            onChange={setNote}
          />
          <HStack gap={2} hAlign="end">
            <Button label={t("common.cancel")} variant="ghost" onClick={close} />
            <Button
              label={t("inventory.adjust_submit")}
              variant="primary"
              isLoading={isPending}
              isDisabled={!canSave}
              onClick={save}
            />
          </HStack>
        </VStack>
      </Dialog>
    </>
  );
}
