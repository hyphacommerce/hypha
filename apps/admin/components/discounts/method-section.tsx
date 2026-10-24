"use client";

// How buyers get the discount: a code they type, or applied by itself; and its title,
// which buyers see either way.

import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";

import { generateCode } from "@/lib/discounts/codes";

import { type SectionProps, fieldStatus } from "./fields";

export function MethodSection({
  draft,
  errors,
  onChange,
  isReadOnly,
  isNew,
}: SectionProps & {
  /** Only a new discount types its first code here; an existing one has a Codes tab. */
  isNew: boolean;
}) {
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Method
        </Text>
        <RadioList
          label="How buyers get it"
          value={draft.method}
          onChange={(method) =>
            onChange({ ...draft, method: method === "automatic" ? "automatic" : "codes" }, [
              "method",
              "code",
            ])
          }
          isDisabled={isReadOnly || !isNew}
        >
          <RadioListItem
            value="codes"
            label="Discount code"
            description="Buyers enter a code in the cart or at checkout. One discount can have many codes."
          />
          <RadioListItem
            value="automatic"
            label="Automatic"
            description="Applies by itself to every cart that qualifies."
          />
        </RadioList>
        <TextInput
          label="Title"
          description="Buyers see it in the cart, at checkout and in their emails."
          placeholder={draft.method === "codes" ? "Welcome offer" : "Free shipping over €50"}
          value={draft.title}
          onChange={(title) => onChange({ ...draft, title }, ["title"])}
          isDisabled={isReadOnly}
          status={fieldStatus(errors.title)}
          width="100%"
        />
        {draft.method === "codes" && isNew && (
          <HStack gap={2} vAlign="end" wrap="wrap">
            <TextInput
              label="Code"
              isOptional
              description="Leave it blank to add or generate codes once it's created."
              placeholder="WELCOME10"
              value={draft.code}
              onChange={(code) => onChange({ ...draft, code }, ["code"])}
              isDisabled={isReadOnly}
              status={fieldStatus(errors.code)}
              width={280}
            />
            {!isReadOnly && (
              <Button
                label="Generate"
                variant="secondary"
                onClick={() => onChange({ ...draft, code: generateCode() }, ["code"])}
              />
            )}
          </HStack>
        )}
      </VStack>
    </Card>
  );
}
