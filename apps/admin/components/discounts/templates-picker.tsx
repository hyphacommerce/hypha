"use client";

// "Create discount"'s starting points as cards: choosing one hands its draft back.

import { VStack } from "@astryxdesign/core/Layout";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Text } from "@astryxdesign/core/Text";

import type { DiscountDraft } from "@/lib/discounts/draft";
import { TEMPLATES, type TemplateKey } from "@/lib/discounts/templates";

export function TemplatesPicker({
  value,
  onChoose,
}: {
  value: TemplateKey | null;
  onChoose: (key: TemplateKey, draft: DiscountDraft) => void;
}) {
  return (
    <div className="template-grid">
      {TEMPLATES.map((t) => (
        <SelectableCard
          key={t.key}
          label={t.label}
          isSelected={value === t.key}
          onChange={(isSelected) => isSelected && onChoose(t.key, t.make())}
          padding={3}
        >
          <VStack gap={1}>
            <Text weight="semibold">{t.label}</Text>
            <Text type="supporting" color="secondary">
              {t.description}
            </Text>
          </VStack>
        </SelectableCard>
      ))}
    </div>
  );
}
