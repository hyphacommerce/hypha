"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";

import { RichTextField } from "@/components/rich-text-field";

import type { Draft } from "./draft";

/** Title, description and handle, part of the editor's draft (saved with the bar). */
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
        <TextInput label="Title" value={draft.title} onChange={(title) => onChange({ title })} />
        <RichTextField
          label="Description"
          description="Shown on the product page. Links open in a new tab."
          isOptional
          minHeight={200}
          value={draft.description}
          onChange={(description) => onChange({ description })}
        />
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
