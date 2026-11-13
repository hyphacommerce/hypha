"use client";

// "Create discount"'s starting points as cards: choosing one hands its draft back.

import { VStack } from "@astryxdesign/core/Layout";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Text } from "@astryxdesign/core/Text";

import type { DiscountDraft } from "@/lib/discounts/draft";
import { TEMPLATES, type TemplateKey } from "@/lib/discounts/templates";
import { useT } from "@/lib/use-t";

export function TemplatesPicker({
  value,
  onChoose,
}: {
  value: TemplateKey | null;
  onChoose: (key: TemplateKey, draft: DiscountDraft) => void;
}) {
  const t = useT<"discounts">();
  return (
    <div className="template-grid">
      {TEMPLATES.map((template) => (
        <SelectableCard
          key={template.key}
          label={t(`discounts.template_${template.key}`)}
          isSelected={value === template.key}
          onChange={(isSelected) => isSelected && onChoose(template.key, template.make())}
          padding={3}
        >
          <VStack gap={1}>
            <Text weight="semibold">{t(`discounts.template_${template.key}`)}</Text>
            <Text type="supporting" color="secondary">
              {t(`discounts.template_${template.key}_description`)}
            </Text>
          </VStack>
        </SelectableCard>
      ))}
    </div>
  );
}
