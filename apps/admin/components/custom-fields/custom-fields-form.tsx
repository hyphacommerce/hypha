"use client";

// An owner's custom fields as a form: one input per definition, in the definitions'
// order. Controlled: the card around it holds the inputs and saves them, with its own
// Save (`CustomFieldsCard`) or the product editor's bar.

import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

import type { Definition, FieldInput, OwnerType } from "@/lib/custom-fields";
import { useT } from "@/lib/use-t";

import { CustomFieldInput } from "./field-input";

export function CustomFieldsForm({
  definitions,
  inputs,
  onChange,
  problems,
  isWritable,
  redacted,
}: {
  definitions: Definition[];
  inputs: Record<string, FieldInput>;
  onChange: (key: string, input: FieldInput) => void;
  /** What's wrong with each field, by key. */
  problems?: Record<string, string>;
  /** Whether the session may change a field's value. */
  isWritable: (definition: Definition) => boolean;
  /** Fields left out for this session (`custom_fields.<key>`). */
  redacted?: string[];
}) {
  const t = useT<"custom_fields">();
  const hidden = new Set((redacted ?? []).map((name) => name.replace(/^custom_fields\./, "")));
  return (
    <VStack gap={4}>
      {definitions.map((definition) => {
        const input = inputs[definition.field];
        if (!input) return null;
        if (hidden.has(definition.field)) {
          return (
            <VStack key={definition.field} gap={0.5}>
              <Text weight="medium">{definition.name}</Text>
              <Text type="supporting" color="secondary">
                {t("custom_fields.form_hidden_personal")}
              </Text>
            </VStack>
          );
        }
        return (
          <CustomFieldInput
            key={definition.field}
            definition={definition}
            input={input}
            onChange={(next) => onChange(definition.field, next)}
            isDisabled={!isWritable(definition)}
            error={problems?.[definition.field]}
          />
        );
      })}
    </VStack>
  );
}

/** The card the fields sit in, with its title, a link to the definitions and `actions`. */
export function CustomFieldsFrame({
  ownerType,
  error,
  actions,
  children,
}: {
  ownerType: OwnerType;
  error?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const t = useT<"custom_fields">();
  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("custom_fields.title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t.rich("custom_fields.card_hint", {
              owner: ownerType,
              link: (chunks, key) => (
                <Link key={key} href={`/custom-fields?owner=${ownerType}`}>
                  {chunks}
                </Link>
              ),
            })}
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {children}
        {/* Below the fields: the card often sits in a narrow column, where header
            buttons would wrap above them. */}
        {actions && <HStack hAlign="end">{actions}</HStack>}
      </VStack>
    </Card>
  );
}
