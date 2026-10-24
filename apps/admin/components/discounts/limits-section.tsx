"use client";

// How often the discount may be used: in total, and once per customer. Uses are counted
// when an order is placed, and a cancelled order doesn't give its use back.

import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";

import { type SectionProps, fieldStatus } from "./fields";

export function LimitsSection({ draft, errors, onChange, isReadOnly }: SectionProps) {
  const limits = draft.limits;
  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Limits
          </Text>
          <Text type="supporting" color="secondary">
            A use is counted when an order is placed with it. Cancelling the order doesn&apos;t give
            it back: raise the limit instead.
          </Text>
        </VStack>
        <TextInput
          label="Total uses"
          isOptional
          description="Across every buyer and code. Blank: no limit."
          value={limits.totalUses}
          onChange={(totalUses) =>
            onChange({ ...draft, limits: { ...limits, totalUses } }, ["limits.totalUses"])
          }
          isDisabled={isReadOnly}
          status={fieldStatus(errors["limits.totalUses"])}
          width={200}
        />
        <CheckboxInput
          label="Once per customer"
          description="Counted by the buyer's email, so a guest checkout counts too."
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
