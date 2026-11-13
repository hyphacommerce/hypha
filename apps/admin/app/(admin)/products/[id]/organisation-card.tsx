"use client";

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Tokenizer } from "@astryxdesign/core/Tokenizer";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";

import { useT } from "@/lib/use-t";

import type { Draft } from "./draft";

/** Tags are typed in, not picked from a list: the source offers nothing, `hasCreate` adds. */
const noSuggestions: SearchSource = { search: () => [], bootstrap: () => [] };

const asItems = (tags: string[]): SearchableItem[] => tags.map((tag) => ({ id: tag, label: tag }));

const firstWithLabel = (items: SearchableItem[], label: string) =>
  items.findIndex((item) => item.label.toLowerCase() === label.toLowerCase());

/**
 * How the product is organised: vendor, product type and tags, which shoppers filter
 * by. Part of the editor's draft.
 */
export function OrganisationCard({
  draft,
  onChange,
}: {
  draft: Draft;
  onChange: (change: Partial<Draft>) => void;
}) {
  const t = useT<"products">();
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          {t("products.organisation_title")}
        </Text>
        <TextInput
          label={t("products.vendor")}
          isOptional
          value={draft.vendor}
          onChange={(vendor) => onChange({ vendor })}
        />
        <TextInput
          label={t("products.organisation_product_type")}
          isOptional
          placeholder={t("products.organisation_product_type_placeholder")}
          value={draft.productType}
          onChange={(productType) => onChange({ productType })}
        />
        <Tokenizer
          label={t("products.organisation_tags")}
          description={t("products.organisation_tags_description")}
          isOptional
          placeholder={t("products.organisation_tags_placeholder")}
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
