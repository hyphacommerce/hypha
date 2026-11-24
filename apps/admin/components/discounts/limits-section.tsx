"use client";

// How often the discount may be used: in total, and once per customer. Uses are counted
// when an order is placed, and a cancelled order doesn't give its use back.

import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";

import { useT } from "@/lib/use-t";

import { type SectionProps, fieldStatus } from "./fields";

export function LimitsSection({ draft, errors, onChange, isReadOnly }: SectionProps) {
  const t = useT<"discounts">();
  const limits = draft.limits;
  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("discounts.limits_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("discounts.limits_hint")}
          </Text>
        </VStack>
        <TextInput
          label={t("discounts.limits_total")}
          isOptional
          description={t("discounts.limits_total_hint")}
          value={limits.totalUses}
          onChange={(totalUses) =>
            onChange({ ...draft, limits: { ...limits, totalUses } }, ["limits.totalUses"])
          }
          isDisabled={isReadOnly}
          status={fieldStatus(errors["limits.totalUses"])}
          width={200}
        />
        <CheckboxInput
          label={t("discounts.limits_once")}
          description={t("discounts.limits_once_hint")}
          value={limits.oncePerCustomer}
          onChange={(oncePerCustomer) =>
            onChange({ ...draft, limits: { ...limits, oncePerCustomer } }, [
              "limits.oncePerCustomer",
            ])
          }
          isDisabled={isReadOnly}
        />
      </VStack>
    </Card>
  );
}
