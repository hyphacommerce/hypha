"use client";

// A customer's labels: the well-known `custom.tags` field rendered as chips under the
// name, each a link to the customers filtered by it, with an editor offering the labels
// in use. Where the field doesn't exist yet, staff who may define fields are offered to
// create it from the API's suggestion.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";
import { Tokenizer } from "@astryxdesign/core/Tokenizer";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";
import { PencilSquareIcon, TagIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useMemo, useState, useTransition } from "react";

import { saveCustomFields } from "@/components/custom-fields/actions";
import { type Definition, type FieldsResponse, filterParam } from "@/lib/custom-fields";
import { textItems } from "@/lib/well-known-fields";

import { addWellKnownField, labelsInUse } from "../actions";

/** Where the customers list filtered by `label` is. */
export function labelHref(definition: Definition, label: string): string {
  return `/customers?${encodeURIComponent(filterParam(definition))}=${encodeURIComponent(label)}`;
}

export function CustomerLabels({
  customerId,
  definition,
  fields,
  canEdit,
  canDefine,
}: {
  customerId: string;
  /** The well-known labels field, when the merchant has it. */
  definition: Definition | undefined;
  /** The customer's values, when the session may read them. */
  fields: FieldsResponse | undefined;
  /** `customers:write`: the labels can change. */
  canEdit: boolean;
  /** `custom_fields:write`: the field can be created. */
  canDefine: boolean;
}) {
  const [isEditing, setIsEditing] = useState(false);
  if (!definition) {
    return canDefine ? <AddWellKnown which="labels" label="Add labels" /> : null;
  }
  if (!fields) return null;
  if (fields.redacted?.includes(`custom_fields.${definition.field}`)) {
    return (
      <Text type="supporting" color="secondary">
        Labels: hidden from your role.
      </Text>
    );
  }
  const labels = textItems(fields.custom_fields[definition.field]);
  return (
    <HStack gap={1} vAlign="center" wrap="wrap">
      {labels.map((label) => (
        <Token key={label} label={label} size="sm" href={labelHref(definition, label)} />
      ))}
      {labels.length === 0 && (
        <Text type="supporting" color="secondary">
          No labels
        </Text>
      )}
      {canEdit && (
        <IconButton
          label="Edit labels"
          icon={<PencilSquareIcon />}
          size="sm"
          variant="ghost"
          onClick={() => setIsEditing(true)}
        />
      )}
      {isEditing && (
        <LabelsDialog
          customerId={customerId}
          definition={definition}
          labels={labels}
          version={fields.versions[definition.field]}
          onClose={() => setIsEditing(false)}
        />
      )}
    </HStack>
  );
}

/** A ghost button creating a well-known field; the page re-renders with it. */
export function AddWellKnown({ which, label }: { which: "labels" | "note"; label: string }) {
  const router = useRouter();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <VStack gap={1}>
      <Button
        label={label}
        variant="ghost"
        size="sm"
        icon={<TagIcon />}
        isLoading={isPending}
        onClick={() =>
          startTransition(async () => {
            const result = await addWellKnownField(which);
            if (result.error) setError(result.error);
            else router.refresh();
          })
        }
      />
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

const toItem = (label: string): SearchableItem => ({ id: label.toLowerCase(), label });

/**
 * The labels in use as a search source, read once when the editor opens. Labels the
 * customer already has may be offered again; the editor drops repeats on change.
 */
function useLabelSource(definition: Definition): SearchSource {
  return useMemo(() => {
    const loaded = labelsInUse(definition.id);
    const offered = (all: string[], query: string) => {
      const q = query.trim().toLowerCase();
      return all.filter((l) => q === "" || l.toLowerCase().includes(q)).map(toItem);
    };
    return {
      search: async (query) => offered(await loaded, query),
      bootstrap: async () => offered(await loaded, ""),
    };
  }, [definition.id]);
}

function LabelsDialog({
  customerId,
  definition,
  labels,
  version,
  onClose,
}: {
  customerId: string;
  definition: Definition;
  labels: string[];
  /** The value's version, for `if_versions`; none when the customer has no labels yet. */
  version: number | undefined;
  onClose: () => void;
}) {
  const router = useRouter();
  const [items, setItems] = useState(() => labels.map(toItem));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const source = useLabelSource(definition);
  const max = definition.validations.max_items ?? 128;

  const submit = () =>
    startTransition(async () => {
      const next = items.map((i) => i.label.trim()).filter((l) => l !== "");
      const result = await saveCustomFields("customer", customerId, {
        custom_fields: { [definition.field]: next.length > 0 ? next : null },
        if_versions: version === undefined ? {} : { [definition.field]: version },
      });
      if (result.error) {
        setError(result.problems?.[definition.field] ?? result.error);
        return;
      }
      router.refresh();
      onClose();
    });

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title="Labels"
        subtitle="Words to find this customer by again. Type a new one, or pick one in use."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Tokenizer
          label={definition.name}
          isLabelHidden
          placeholder="Add a label…"
          searchSource={source}
          hasCreate
          hasEntriesOnFocus
          debounceMs={0}
          maxEntries={max}
          value={items}
          onChange={(next) => {
            // Labels are unique ignoring case: a repeat is dropped before the API sees it.
            const seen = new Set<string>();
            setItems(
              next
                .map((i) => ({ id: i.label.trim().toLowerCase(), label: i.label.trim() }))
                .filter((i) => i.id !== "" && !seen.has(i.id) && seen.add(i.id)),
            );
            setError(undefined);
          }}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Save" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
