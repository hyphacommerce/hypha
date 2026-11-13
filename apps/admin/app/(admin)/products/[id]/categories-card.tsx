"use client";

import { Card } from "@astryxdesign/core/Card";
import { Link } from "@astryxdesign/core/Link";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Text } from "@astryxdesign/core/Text";
import { Token } from "@astryxdesign/core/Token";

import { type CategoryNode, flatten } from "@/lib/categories";
import { useT } from "@/lib/use-t";

import type { Draft } from "./draft";

/**
 * The categories the product is in: part of the editor's draft, saved with the bar
 * like its details and publishing.
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
  const t = useT<"products">();
  const options = flatten(tree).map((row) => ({
    value: row.id,
    label: row.path,
  }));

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("common.categories")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("products.categories_hint")}{" "}
            <Link href="/products/categories">{t("products.categories_manage")}</Link>
          </Text>
        </VStack>
        {options.length === 0 ? (
          <Text color="secondary">{t("products.categories_none")}</Text>
        ) : (
          <VStack gap={2}>
            {/* The picker shows a count, the chips below the choice: badges inside the
                trigger overflowed the card with a few long paths. */}
            <MultiSelector
              label={t("common.categories")}
              isLabelHidden
              placeholder={t("products.categories_placeholder")}
              options={options}
              value={draft.categories}
              onChange={(categories) => onChange({ categories: categories.toSorted() })}
              hasSearch
              triggerDisplay="count"
              formatValue={(items) => t("products.categories_count", { count: items.length })}
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
