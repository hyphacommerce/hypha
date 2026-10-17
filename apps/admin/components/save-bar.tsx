"use client";

// The bar a page with unsaved changes shows (design note 0024, section 6): sticky at
// the top, with Discard and Save, and the save's error under them. The product editor,
// a market and the store settings save this way, every section of the page at once.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";

export function SaveBar({
  isDirty,
  isPending,
  error,
  canSave = true,
  onSave,
  onDiscard,
}: {
  isDirty: boolean;
  isPending: boolean;
  /** Why the last save failed; keeps the bar up until it's dealt with. */
  error?: string;
  /** False while the draft breaks a rule the page shows (an empty name). */
  canSave?: boolean;
  onSave: () => void;
  onDiscard: () => void;
}) {
  if (!isDirty && !error) return null;
  return (
    <div className="save-bar">
      <HStack gap={3} hAlign="between" vAlign="center" wrap="wrap">
        <Text weight="medium">{isDirty ? "Unsaved changes" : "Not saved"}</Text>
        <HStack gap={2}>
          <Button label="Discard" variant="ghost" isDisabled={isPending} onClick={onDiscard} />
          <Button
            label="Save"
            variant="primary"
            isLoading={isPending}
            isDisabled={!isDirty || !canSave}
            onClick={onSave}
          />
        </HStack>
      </HStack>
      {error && (
        <div style={{ marginTop: 12 }}>
          <Banner status="error" title={error} />
        </div>
      )}
    </div>
  );
}
