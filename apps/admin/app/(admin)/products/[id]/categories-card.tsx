"use client";

import { Card } from "@astryxdesign/core/Card";
import { Link } from "@astryxdesign/core/Link";
import { VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Text } from "@astryxdesign/core/Text";

import { type CategoryNode, flatten } from "@/lib/categories";

import type { Draft } from "./draft";

/**
 * The categories the product is in (design note 0022, section 5): part of the editor's
 * draft, saved with the bar like its details, unlike publishing, which acts at once.
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
            Where shoppers find it when browsing. A category shows its subcategories'
            products too. <Link href="/products/categories">Manage categories</Link>
          </Text>
        </VStack>
        {options.length === 0 ? (
          <Text color="secondary">No categories yet.</Text>
        ) : (
          <MultiSelector
            label="Categories"
            isLabelHidden
            placeholder="None"
            options={options}
            value={draft.categories}
            onChange={(categories) => onChange({ categories: [...categories].sort() })}
            hasSearch
            triggerDisplay="badges"
            maxBadges={6}
          />
        )}
      </VStack>
    </Card>
  );
}
