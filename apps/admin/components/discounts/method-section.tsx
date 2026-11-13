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
import { useT } from "@/lib/use-t";

import { type SectionProps, fieldStatus } from "./fields";

/** What a code looks like: a code, the same in every language. */
const CODE_EXAMPLE = "WELCOME10";

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
  const t = useT<"discounts">();
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          {t("discounts.method_title")}
        </Text>
        <RadioList
          label={t("discounts.method_label")}
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
            label={t("discounts.method_code")}
            description={t("discounts.method_code_description")}
          />
          <RadioListItem
            value="automatic"
            label={t("discounts.method_automatic")}
            description={t("discounts.method_automatic_description")}
          />
        </RadioList>
        <TextInput
          label={t("common.title")}
          description={t("discounts.method_title_hint")}
          placeholder={
            draft.method === "codes"
              ? t("discounts.method_title_placeholder_codes")
              : t("discounts.method_title_placeholder_automatic")
          }
          value={draft.title}
          onChange={(title) => onChange({ ...draft, title }, ["title"])}
          isDisabled={isReadOnly}
          status={fieldStatus(errors.title)}
          width="100%"
        />
        {draft.method === "codes" && isNew && (
          <HStack gap={2} vAlign="end" wrap="wrap">
            <TextInput
              label={t("common.code")}
              isOptional
              description={t("discounts.method_code_hint")}
              placeholder={CODE_EXAMPLE}
              value={draft.code}
              onChange={(code) => onChange({ ...draft, code }, ["code"])}
              isDisabled={isReadOnly}
              status={fieldStatus(errors.code)}
              width={280}
            />
            {!isReadOnly && (
              <Button
                label={t("discounts.codes_generate")}
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
