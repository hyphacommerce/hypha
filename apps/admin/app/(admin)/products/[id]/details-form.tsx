"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";

import { RichTextField } from "@/components/rich-text-field";
import { useT } from "@/lib/use-t";

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
  const t = useT<"products">();
  return (
    <Card>
      <VStack gap={4}>
        <TextInput
          label={t("common.title")}
          value={draft.title}
          onChange={(title) => onChange({ title })}
        />
        <RichTextField
          label={t("common.description")}
          description={t("products.details_description_hint")}
          isOptional
          minHeight={200}
          value={draft.description}
          onChange={(description) => onChange({ description })}
        />
        <TextInput
          label={t("common.handle")}
          description={t("products.handle_description")}
          value={draft.handle}
          onChange={(handle) => onChange({ handle })}
          status={
            isActive && draft.handle !== savedHandle
              ? {
                  type: "warning",
                  message: t("products.details_handle_live_warning"),
                }
              : undefined
          }
          statusVariant="detached"
        />
      </VStack>
    </Card>
  );
}
