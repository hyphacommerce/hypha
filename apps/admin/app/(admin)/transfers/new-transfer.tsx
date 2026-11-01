"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useState, useTransition } from "react";

import { createTransfer, transferLocations } from "./actions";

/** Starts a draft: where the stock leaves from and goes to. Lines are added on its page. */
export function NewTransfer() {
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
      <Button label="New transfer" variant="primary" onClick={open} />
      <Dialog isOpen={isOpen} onOpenChange={(o) => !o && close()} width={460} purpose="form">
        <DialogHeader
          title="New transfer"
          subtitle="What moves and when it's due come next, on its page."
          onOpenChange={(o) => !o && close()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          {locations && locations.length < 2 ? (
            <Text type="supporting" color="secondary">
              Add a second location first: a transfer moves stock from one to another.
            </Text>
          ) : (
            <>
              <Selector
                label="From"
                options={options}
                value={origin || undefined}
                onChange={setOrigin}
                placeholder={locations ? "Choose a location" : "Loading…"}
              />
              <Selector
                label="To"
                options={options}
                value={destination || undefined}
                onChange={setDestination}
                placeholder={locations ? "Choose a location" : "Loading…"}
                status={
                  origin && origin === destination
                    ? { type: "error", message: "Choose another location than where it's from" }
                    : undefined
                }
              />
            </>
          )}
          <HStack gap={2} hAlign="end">
            <Button label="Cancel" variant="ghost" onClick={close} />
            <Button
              label="Write draft"
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
