"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState, useTransition } from "react";

import { updateProduct } from "../actions";

/** Title and handle, saved together (one PATCH). */
export function DetailsForm({
  id,
  title: savedTitle,
  handle: savedHandle,
  isActive,
}: {
  id: string;
  title: string;
  handle: string;
  isActive: boolean;
}) {
  const [title, setTitle] = useState(savedTitle);
  const [handle, setHandle] = useState(savedHandle);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  // The title as the API stores it (trimmed), so saving " Shirt " leaves nothing unsaved.
  const isDirty = title.trim() !== savedTitle || handle !== savedHandle;
  const save = () => {
    if (!isDirty) return;
    startTransition(async () => {
      const result = await updateProduct(id, { title, handle });
      setError(result.error);
      if (!result.error) setTitle(title.trim());
    });
  };

  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Details
        </Text>
        {error && <Banner status="error" title={error} />}
        <TextInput label="Title" value={title} onChange={setTitle} onEnter={save} />
        <TextInput
          label="Handle"
          description="The product's address in the storefront: lowercase letters, digits and dashes."
          value={handle}
          onChange={setHandle}
          onEnter={save}
          status={
            isActive && handle !== savedHandle
              ? {
                  type: "warning",
                  message: "This product is live: links to its old address will stop working.",
                }
              : undefined
          }
          statusVariant="detached"
        />
        <HStack gap={2} hAlign="end">
          {isDirty && (
            <Button
              label="Discard"
              variant="ghost"
              onClick={() => {
                setTitle(savedTitle);
                setHandle(savedHandle);
                setError(undefined);
              }}
            />
          )}
          <Button
            label="Save"
            variant="primary"
            isDisabled={!isDirty}
            isLoading={isPending}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Card>
  );
}
