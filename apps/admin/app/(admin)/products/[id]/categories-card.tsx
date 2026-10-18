"use client";

import { Card } from "@astryxdesign/core/Card";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";

import { type CategoryNode, flatten } from "@/lib/categories";

import type { Draft } from "./draft";

/**
 * The categories the product is in (design note 0022, section 5): part of the editor's
 * draft, saved with the bar like its details and publishing.
 */
export function CategoriesCard({
  tree,
  draft,
  onChange,
}: {
  tree: CategoryNode[];
  draft: Draft;
  onChange: (change: Partial<Draft>) => void;
}) {
  const options = flatten(tree).map((row) => ({
    value: row.id,
    label: row.path,
  }));

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Categories
          </Text>
          <Text type="supporting" color="secondary">
            Where shoppers find it when browsing. A category shows its subcategories' products too.{" "}
            <Link href="/products/categories">Manage categories</Link>
          </Text>
        </VStack>
        {options.length === 0 ? (
          <Text color="secondary">No categories yet.</Text>
        ) : (
          <VStack gap={2}>
            {/* The picker shows a count, the chips below the choice: badges inside the
                trigger overflowed the card with a few long paths. */}
            <MultiSelector
              label="Categories"
              isLabelHidden
              placeholder="Add to categories"
              options={options}
              value={draft.categories}
              onChange={(categories) => onChange({ categories: categories.toSorted() })}
              hasSearch
              triggerDisplay="count"
              formatValue={(items) =>
                items.length === 1 ? "1 category" : `${items.length} categories`
              }
              width="100%"
            />
            {draft.categories.length > 0 && (
              <HStack gap={1} wrap="wrap">
                {draft.categories.map((id) => (
                  <Token
                    key={id}
                    size="sm"
                    label={options.find((o) => o.value === id)?.label ?? id}
                    onRemove={() =>
                      onChange({ categories: draft.categories.filter((c) => c !== id) })
                    }
                  />
                ))}
              </HStack>
            )}
          </VStack>
        )}
      </VStack>
    </Card>
  );
}
