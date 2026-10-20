"use client";

// A "Custom fields" card with its own Save (design note 0033, §9), for the pages that
// have no draft of their own: categories, customers, orders, and a variant's dialog.

import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import {
  type Definition,
  type FieldsResponse,
  type OwnerType,
  fieldsChanged,
  fieldsPatch,
  inputsOf,
  isWritable,
} from "@/lib/custom-fields";

import { type FieldsPatch, saveCustomFields } from "./actions";
import { CustomFieldsForm, CustomFieldsFrame } from "./custom-fields-form";

/**
 * The editing state of an owner's fields: the inputs, what's saved, problems, and a
 * save that sends only what changed.
 */
export function useCustomFields({
  ownerType,
  ownerId,
  definitions,
  fields,
  currency,
}: {
  ownerType: OwnerType;
  ownerId: string;
  definitions: Definition[];
  fields: FieldsResponse;
  currency: string;
}) {
  const currencies = useCurrencies();
  const [saved, setSaved] = useState(fields);
  const [inputs, setInputs] = useState(() => inputsOf(definitions, fields, currencies, currency));
  const [problems, setProblems] = useState<Record<string, string>>({});
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  // New values from the server (after a save elsewhere, a refresh): take them in unless
  // there are unsaved edits ("adjust state while rendering").
  const [seen, setSeen] = useState(fields);
  const isDirty = fieldsChanged(definitions, inputs, saved, currencies);
  if (fields !== seen) {
    setSeen(fields);
    setSaved(fields);
    if (!isDirty) setInputs(inputsOf(definitions, fields, currencies, currency));
  }

  const edit = (key: string, input: (typeof inputs)[string]) => {
    setInputs((before) => ({ ...before, [key]: input }));
    setProblems((before) => {
      const rest = { ...before };
      delete rest[key];
      return rest;
    });
    setError(undefined);
  };
  const discard = () => {
    setInputs(inputsOf(definitions, saved, currencies, currency));
    setProblems({});
    setError(undefined);
  };
  /** The patch saving sends (`null`: nothing changed), or an error after marking the problems. */
  const prepare = (): { patch: FieldsPatch | null } | { error: string } => {
    const built = fieldsPatch(definitions, inputs, saved, currencies);
    if ("problems" in built) {
      setProblems(built.problems);
      return { error: "Some custom fields need fixing first." };
    }
    return { patch: built.patch };
  };
  /** Takes in what a save stored. */
  const accept = (stored: FieldsResponse) => {
    setSaved(stored);
    setSeen(stored);
    setInputs(inputsOf(definitions, stored, currencies, currency));
    setProblems({});
    setError(undefined);
  };
  /** Shows a failed save's problems next to their fields. */
  const reject = (found: Record<string, string>) => setProblems(found);

  const save = (onSaved?: () => void) => {
    const built = prepare();
    if ("error" in built) {
      setError(built.error);
      return;
    }
    const patch = built.patch;
    if (!patch) {
      onSaved?.();
      return;
    }
    startTransition(async () => {
      const result = await saveCustomFields(ownerType, ownerId, patch);
      if (result.error) {
        setError(result.error);
        setProblems(result.problems ?? {});
        return;
      }
      if (result.fields) accept(result.fields);
      onSaved?.();
    });
  };
  return {
    inputs,
    saved,
    problems,
    error,
    isDirty,
    isPending,
    edit,
    discard,
    save,
    prepare,
    accept,
    reject,
  };
}

export function CustomFieldsCard({
  ownerType,
  ownerId,
  definitions,
  fields,
  scopes,
  currency = "EUR",
}: {
  ownerType: OwnerType;
  ownerId: string;
  /** The owner type's definitions, in order. */
  definitions: Definition[];
  fields: FieldsResponse;
  /** The session's scopes, for which fields it may change. */
  scopes: readonly string[] | null;
  /** The currency a new money value starts in. */
  currency?: string;
}) {
  const state = useCustomFields({ ownerType, ownerId, definitions, fields, currency });
  const writable = (d: Definition) => isWritable(d, ownerType, scopes);
  const canWrite = definitions.some(writable);

  return (
    <CustomFieldsFrame
      ownerType={ownerType}
      error={state.error}
      actions={
        canWrite && (
          <HStack gap={2}>
            {state.isDirty && (
              <Button label="Discard" variant="ghost" size="sm" onClick={state.discard} />
            )}
            <Button
              label="Save"
              variant="primary"
              size="sm"
              isDisabled={!state.isDirty}
              isLoading={state.isPending}
              onClick={() => state.save()}
            />
          </HStack>
        )
      }
    >
      <CustomFieldsForm
        definitions={definitions}
        inputs={state.inputs}
        onChange={state.edit}
        problems={state.problems}
        isWritable={writable}
        redacted={state.saved.redacted ?? undefined}
      />
    </CustomFieldsFrame>
  );
}
