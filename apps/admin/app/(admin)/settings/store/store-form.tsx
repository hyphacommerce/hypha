"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { startTransition, useActionState, useState } from "react";

import { renameStore } from "./actions";

export function StoreForm({ name: current }: { name: string }) {
  const [name, setName] = useState(current);
  const [state, save, isPending] = useActionState(renameStore, {});
  const isChanged = name.trim() !== current;

  const submit = () => {
    if (!isChanged || !name.trim()) return;
    startTransition(() => save(name));
  };

  return (
    <VStack gap={4}>
      {state.error && <Banner status="error" title={state.error} />}
      {state.saved && !isChanged && <Banner status="success" title="Saved." />}
      <TextInput
        label="Store name"
        description="Shown to staff here and to shoppers in the storefront. Up to 100 characters."
        value={name}
        onChange={setName}
        onEnter={submit}
      />
      <HStack hAlign="end">
        <Button
          label="Save"
          variant="primary"
          isLoading={isPending}
          isDisabled={!isChanged || !name.trim()}
          onClick={submit}
        />
      </HStack>
    </VStack>
  );
}
