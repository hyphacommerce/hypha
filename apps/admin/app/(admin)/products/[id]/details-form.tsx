"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";

import type { Draft } from "./draft";

/** Title and handle, part of the editor's draft (saved with the bar). */
export function DetailsForm({
  draft,
  savedHandle,
  isActive,
  onChange,
}: {
  draft: Draft;
  savedHandle: string;
  isActive: boolean;
  onChange: (change: Partial<Draft>) => void;
}) {
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Details
        </Text>
        <TextInput label="Title" value={draft.title} onChange={(title) => onChange({ title })} />
        <TextInput
          label="Handle"
          description="The product's address in the storefront: lowercase letters, digits and dashes."
          value={draft.handle}
          onChange={(handle) => onChange({ handle })}
          status={
            isActive && draft.handle !== savedHandle
              ? {
                  type: "warning",
                  message: "This product is live: links to its old address will stop working.",
                }
              : undefined
          }
          statusVariant="detached"
        />
      </VStack>
    </Card>
  );
}
