"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { createTransfer, transferLocations } from "./actions";

/** Starts a draft: where the stock leaves from and goes to. Lines are added on its page. */
export function NewTransfer() {
  const t = useT<"transfers">();
  const [isOpen, setIsOpen] = useState(false);
  const [locations, setLocations] = useState<{ id: string; name: string }[]>();
  const [origin, setOrigin] = useState("");
  const [destination, setDestination] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const open = () => {
    setIsOpen(true);
    startTransition(async () => {
      const found = await transferLocations();
      setLocations(found);
      if (!origin && found[0]) setOrigin(found[0].id);
      if (!destination && found[1]) setDestination(found[1].id);
    });
  };
  const close = () => {
    setIsOpen(false);
    setError(undefined);
  };
  const canSave = origin !== "" && destination !== "" && origin !== destination;
  const save = () => {
    if (!canSave) return;
    startTransition(async () => {
      const result = await createTransfer({ origin, destination });
      if (result?.error) setError(result.error);
    });
  };
  const options = (locations ?? []).map((l) => ({ value: l.id, label: l.name }));
  return (
    <>
      <Button label={t("transfers.new_button")} variant="primary" onClick={open} />
      <Dialog isOpen={isOpen} onOpenChange={(o) => !o && close()} width={460} purpose="form">
        <DialogHeader
          title={t("transfers.new_title")}
          subtitle={t("transfers.new_subtitle")}
          onOpenChange={(o) => !o && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          {locations && locations.length < 2 ? (
            <Text type="supporting" color="secondary">
              {t("transfers.new_needs_locations")}
            </Text>
          ) : (
            <>
              <Selector
                label={t("transfers.from")}
                options={options}
                value={origin || undefined}
                onChange={setOrigin}
                placeholder={locations ? t("common.choose_location") : t("common.loading")}
              />
              <Selector
                label={t("transfers.to")}
                options={options}
                value={destination || undefined}
                onChange={setDestination}
                placeholder={locations ? t("common.choose_location") : t("common.loading")}
                status={
                  origin && origin === destination
                    ? { type: "error", message: t("transfers.same_location") }
                    : undefined
                }
              />
            </>
          )}
          <HStack gap={2} hAlign="end">
            <Button label={t("common.cancel")} variant="ghost" onClick={close} />
            <Button
              label={t("transfers.write_draft")}
              variant="primary"
              isLoading={isPending && !!locations}
              isDisabled={!canSave}
              onClick={save}
            />
          </HStack>
        </VStack>
      </Dialog>
    </>
  );
}
