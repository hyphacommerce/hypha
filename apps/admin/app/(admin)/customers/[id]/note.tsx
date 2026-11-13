"use client";

// The staff note: the well-known `custom.note` field as its own card, personal, so a role
// without `customer_data:profile` sees it hidden. Where the field doesn't exist yet,
// staff who may define fields are offered to create it.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { saveCustomFields } from "@/components/custom-fields/actions";
import type { Definition, FieldsResponse } from "@/lib/custom-fields";
import { useT } from "@/lib/use-t";

import { AddWellKnown } from "./labels";

/** The scope a role needs to see the note: named as it is in every language. */
const PROFILE_SCOPE = "customer_data:profile";

export function NoteCard({
  customerId,
  definition,
  fields,
  canEdit,
  canDefine,
}: {
  customerId: string;
  /** The well-known note field, when the merchant has it. */
  definition: Definition | undefined;
  /** The customer's values, when the session may read them. */
  fields: FieldsResponse | undefined;
  /** `customers:write`. */
  canEdit: boolean;
  /** `custom_fields:write`: the field can be created. */
  canDefine: boolean;
}) {
  const t = useT<"customers">();
  if (!definition) {
    if (!canDefine) return null;
    return (
      <Card>
        <VStack gap={2}>
          <Text type="display-3" as="h2">
            {t("common.note")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("customers.note_missing_description", { scope: PROFILE_SCOPE })}
          </Text>
          <HStack>
            <AddWellKnown which="note" label={t("customers.note_add_field")} />
          </HStack>
        </VStack>
      </Card>
    );
  }
  if (!fields) return null;
  const hidden = fields.redacted?.includes(`custom_fields.${definition.field}`) ?? false;
  const saved = fields.custom_fields[definition.field];
  return (
    <NoteEditor
      key={fields.versions[definition.field] ?? 0}
      customerId={customerId}
      definition={definition}
      saved={typeof saved === "string" ? saved : ""}
      version={fields.versions[definition.field]}
      hidden={hidden}
      canEdit={canEdit}
    />
  );
}

function NoteEditor({
  customerId,
  definition,
  saved,
  version,
  hidden,
  canEdit,
}: {
  customerId: string;
  definition: Definition;
  saved: string;
  version: number | undefined;
  hidden: boolean;
  canEdit: boolean;
}) {
  const router = useRouter();
  const t = useT<"customers">();
  const [text, setText] = useState(saved);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isDirty = text !== saved;

  const save = () =>
    startTransition(async () => {
      const result = await saveCustomFields("customer", customerId, {
        custom_fields: { [definition.field]: text.trim() === "" ? null : text },
        if_versions: version === undefined ? {} : { [definition.field]: version },
      });
      if (result.error) {
        setError(result.problems?.[definition.field] ?? result.error);
        return;
      }
      router.refresh();
    });

  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {definition.name}
        </Text>
        {hidden ? (
          <Text type="supporting" color="secondary">
            {t("customers.note_hidden")}
          </Text>
        ) : canEdit ? (
          <>
            {error && <Banner status="error" title={error} />}
            <TextArea
              label={definition.name}
              isLabelHidden
              description={definition.description ?? undefined}
              placeholder={t("customers.note_placeholder")}
              rows={5}
              maxLength={definition.validations.max_length ?? undefined}
              value={text}
              onChange={(value) => {
                setText(value);
                setError(undefined);
              }}
            />
            {isDirty && (
              <HStack gap={2} hAlign="end">
                <Button
                  label={t("common.discard")}
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setText(saved);
                    setError(undefined);
                  }}
                />
                <Button
                  label={t("common.save")}
                  variant="primary"
                  size="sm"
                  isLoading={isPending}
                  onClick={save}
                />
              </HStack>
            )}
          </>
        ) : saved === "" ? (
          <Text color="secondary">{t("customers.note_empty")}</Text>
        ) : (
          <Text style={{ whiteSpace: "pre-wrap" }}>{saved}</Text>
        )}
      </VStack>
    </Card>
  );
}
