"use client";

// A variant's custom fields (design note 0033, §9): the variants table has no room for
// them, so each saved variant opens them in a dialog, read fresh and saved at once, apart
// from the product's draft (like its stock).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { useEffect, useState, useTransition } from "react";

import { type Definition, type FieldsResponse, isWritable } from "@/lib/custom-fields";

import { loadCustomFields } from "./actions";
import { useCustomFields } from "./custom-fields-card";
import { CustomFieldsForm } from "./custom-fields-form";

export function VariantFieldsCell({
  variantId,
  title,
  currency,
  definitions,
  scopes,
}: {
  variantId: string;
  title: string;
  currency: string;
  definitions: Definition[];
  scopes: readonly string[] | null;
}) {
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button
        label="Fields"
        size="sm"
        variant="ghost"
        tooltip={`Custom fields of ${title}`}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && (
        <VariantFieldsDialog
          variantId={variantId}
          title={title}
          currency={currency}
          definitions={definitions}
          scopes={scopes}
          onClose={() => setIsOpen(false)}
        />
      )}
    </>
  );
}

function VariantFieldsDialog({
  variantId,
  title,
  currency,
  definitions,
  scopes,
  onClose,
}: {
  variantId: string;
  title: string;
  currency: string;
  definitions: Definition[];
  scopes: readonly string[] | null;
  onClose: () => void;
}) {
  const [fields, setFields] = useState<FieldsResponse>();
  const [error, setError] = useState<string>();
  const [, startLoading] = useTransition();

  useEffect(() => {
    startLoading(async () => {
      const result = await loadCustomFields("variant", variantId);
      if (result.error) setError(result.error);
      if (result.fields) setFields(result.fields);
    });
  }, [variantId]);

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title={`Custom fields of ${title}`}
        subtitle="Saved at once, apart from the product's other changes."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {fields ? (
          <VariantFieldsForm
            variantId={variantId}
            currency={currency}
            definitions={definitions}
            scopes={scopes}
            fields={fields}
            onClose={onClose}
          />
        ) : (
          !error && <Skeleton height={120} />
        )}
      </VStack>
    </Dialog>
  );
}

function VariantFieldsForm({
  variantId,
  currency,
  definitions,
  scopes,
  fields,
  onClose,
}: {
  variantId: string;
  currency: string;
  definitions: Definition[];
  scopes: readonly string[] | null;
  fields: FieldsResponse;
  onClose: () => void;
}) {
  const state = useCustomFields({
    ownerType: "variant",
    ownerId: variantId,
    definitions,
    fields,
    currency,
  });
  const writable = (d: Definition) => isWritable(d, "variant", scopes);
  return (
    <>
      {state.error && <Banner status="error" title={state.error} />}
      <CustomFieldsForm
        definitions={definitions}
        inputs={state.inputs}
        onChange={state.edit}
        problems={state.problems}
        isWritable={writable}
        redacted={state.saved.redacted ?? undefined}
      />
      <HStack gap={2} hAlign="end">
        <Button label="Cancel" variant="ghost" onClick={onClose} />
        {definitions.some(writable) && (
          <Button
            label="Save fields"
            variant="primary"
            isLoading={state.isPending}
            isDisabled={!state.isDirty}
            onClick={() => state.save(onClose)}
          />
        )}
      </HStack>
    </>
  );
}
