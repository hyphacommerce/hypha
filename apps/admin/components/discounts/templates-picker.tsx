"use client";

// "Create discount"'s starting points as cards: choosing one hands its draft back; the
// kinds the API doesn't take yet show as coming.

import { Badge } from "@astryxdesign/core/Badge";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Text } from "@astryxdesign/core/Text";

import type { DiscountDraft } from "@/lib/discounts/draft";
import { TEMPLATES, type TemplateKey } from "@/lib/discounts/templates";

export function TemplatesPicker({
  value,
  onChoose,
  isLaterEnabled = false,
}: {
  value: TemplateKey | null;
  onChoose: (key: TemplateKey, draft: DiscountDraft) => void;
  /** Lets the kinds the API doesn't take yet be chosen too. */
  isLaterEnabled?: boolean;
}) {
  return (
    <div className="template-grid">
      {TEMPLATES.map((t) => {
        const isDisabled = t.isLater && !isLaterEnabled;
        return (
          <SelectableCard
            key={t.key}
            label={t.label}
            isSelected={value === t.key}
            isDisabled={isDisabled}
            onChange={(isSelected) => isSelected && onChoose(t.key, t.make())}
            padding={3}
          >
            <VStack gap={1}>
              <HStack gap={2} vAlign="center">
                <Text weight="semibold">{t.label}</Text>
                {isDisabled && <Badge label="Coming" variant="neutral" />}
              </HStack>
              <Text type="supporting" color="secondary">
                {t.description}
              </Text>
            </VStack>
          </SelectableCard>
        );
      })}
    </div>
  );
}
