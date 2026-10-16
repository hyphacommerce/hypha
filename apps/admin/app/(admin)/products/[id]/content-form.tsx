"use client";

import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
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
 * What the product says about itself (design note 0019, section 2): description,
 * vendor, product type and tags, part of the editor's draft. The description is HTML;
 * the API sanitises it, and after saving the form shows what was kept.
 */
export function ContentForm({
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
          Content
        </Text>
        <TextArea
          label="Description"
          description="HTML: paragraphs, headings, lists, links and images. Scripts and styles are removed when it's saved."
          isOptional
          rows={8}
          value={draft.description}
          onChange={(description) => onChange({ description })}
        />
        <HStack gap={4}>
          <TextInput
            label="Vendor"
            isOptional
            value={draft.vendor}
            onChange={(vendor) => onChange({ vendor })}
          />
          <TextInput
            label="Product type"
            isOptional
            value={draft.productType}
            onChange={(productType) => onChange({ productType })}
          />
        </HStack>
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
