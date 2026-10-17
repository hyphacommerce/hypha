"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Tokenizer } from "@astryxdesign/core/Tokenizer";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";

import type { Draft } from "./draft";

/** Tags are typed in, not picked from a list: the source offers nothing, `hasCreate` adds. */
const noSuggestions: SearchSource = { search: () => [], bootstrap: () => [] };

const asItems = (tags: string[]): SearchableItem[] => tags.map((t) => ({ id: t, label: t }));

const firstWithLabel = (items: SearchableItem[], label: string) =>
  items.findIndex((item) => item.label.toLowerCase() === label.toLowerCase());

/**
 * How the product is organised (design note 0019, section 2): vendor, product type and
 * tags, which shoppers filter by. Part of the editor's draft.
 */
export function OrganisationCard({
  draft,
  onChange,
}: {
  draft: Draft;
  onChange: (change: Partial<Draft>) => void;
}) {
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Organisation
        </Text>
        <TextInput
          label="Vendor"
          isOptional
          value={draft.vendor}
          onChange={(vendor) => onChange({ vendor })}
        />
        <TextInput
          label="Product type"
          isOptional
          placeholder="Mug, T-shirt…"
          value={draft.productType}
          onChange={(productType) => onChange({ productType })}
        />
        <Tokenizer
          label="Tags"
          description="Shoppers can filter by them. Up to 250."
          isOptional
          placeholder="Add a tag…"
          searchSource={noSuggestions}
          hasCreate
          debounceMs={0}
          maxEntries={250}
          value={asItems(draft.tags)}
          onChange={(items) =>
            // The API keeps the first spelling of a tag; so does the form.
            onChange({
              tags: items
                .filter((item, i) => firstWithLabel(items, item.label) === i)
                .map((item) => item.label),
            })
          }
        />
      </VStack>
    </Card>
  );
}
