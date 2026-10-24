"use client";

// Which items a product discount covers: every product, chosen products, variants or
// categories (with their subcategories), and whether items already on sale are left out.

import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { Text } from "@astryxdesign/core/Text";
import { useState } from "react";

import { ProductPicker } from "@/components/product-picker";
import type { ItemSetDraft, ItemsScope } from "@/lib/discounts/draft";
import type { FieldErrors } from "@/lib/discounts/validate";

import { FieldError, IdTokens, type Option, type SectionProps, fieldStatus } from "./fields";

const SCOPES: readonly ItemsScope[] = ["all", "products", "variants", "categories"];

const scopeOf = (value: string): ItemsScope => SCOPES.find((s) => s === value) ?? "all";

/** An item set's fields, for a reward or either side of a buy X get Y. */
export function ItemSetFields({
  items,
  path,
  errors,
  onChange,
  isReadOnly,
  categories,
  names,
  onNames,
  onPickVariants,
}: {
  items: ItemSetDraft;
  path: string;
  errors: FieldErrors;
  onChange: (items: ItemSetDraft, paths: string[]) => void;
  isReadOnly?: boolean;
  categories: readonly Option[];
  names: Readonly<Record<string, string>>;
  /** Names of products picked here, for the tokens and the summary. */
  onNames: (names: Record<string, string>) => void;
  /** Opens the page's variant picker; without one, variants can only be removed. */
  onPickVariants?: () => void;
}) {
  const [isPicking, setIsPicking] = useState(false);
  const set = (change: Partial<ItemSetDraft>, field: string) =>
    onChange({ ...items, ...change }, [`${path}.${field}`]);

  return (
    <VStack gap={3}>
      <RadioList
        label="Applies to"
        value={items.scope}
        onChange={(scope) => set({ scope: scopeOf(scope) }, "scope")}
        isDisabled={isReadOnly}
      >
        <RadioListItem value="all" label="Every product" />
        <RadioListItem
          value="categories"
          label="Products in categories"
          description="A category counts the products in its subcategories too."
        />
        <RadioListItem value="products" label="Chosen products" />
        <RadioListItem
          value="variants"
          label="Chosen variants"
          description="Only these variants of their products."
        />
      </RadioList>

      {items.scope === "categories" && (
        <VStack gap={2}>
          <MultiSelector
            label="Categories"
            placeholder="Choose categories"
            options={[...categories]}
            value={items.categories}
            onChange={(chosen) => set({ categories: chosen }, "categories")}
            hasSearch
            triggerDisplay="count"
            formatValue={(chosen) =>
              chosen.length === 1 ? "1 category" : `${chosen.length} categories`
            }
            isDisabled={isReadOnly}
            status={fieldStatus(errors[`${path}.categories`])}
            width="100%"
          />
          <IdTokens
            ids={items.categories}
            names={Object.fromEntries(categories.map((c) => [c.value, c.label]))}
            onRemove={
              isReadOnly
                ? undefined
                : (id) =>
                    set({ categories: items.categories.filter((c) => c !== id) }, "categories")
            }
          />
        </VStack>
      )}

      {items.scope === "products" && (
        <VStack gap={2} hAlign="start">
          <FieldError message={errors[`${path}.products`]} />
          <IdTokens
            ids={items.products}
            names={names}
            onRemove={
              isReadOnly
                ? undefined
                : (id) => set({ products: items.products.filter((p) => p !== id) }, "products")
            }
          />
          {!isReadOnly && (
            <Button
              label="Choose products"
              variant="secondary"
              onClick={() => setIsPicking(true)}
            />
          )}
        </VStack>
      )}

      {items.scope === "variants" && (
        <VStack gap={2} hAlign="start">
          <FieldError message={errors[`${path}.variants`]} />
          <IdTokens
            ids={items.variants}
            names={names}
            onRemove={
              isReadOnly
                ? undefined
                : (id) => set({ variants: items.variants.filter((v) => v !== id) }, "variants")
            }
          />
          {!isReadOnly && onPickVariants && (
            <Button label="Choose variants" variant="secondary" onClick={onPickVariants} />
          )}
        </VStack>
      )}

      <CheckboxInput
        label="Not on items already on sale"
        description="Leaves out items a sale price list has reduced."
        value={items.excludeOnSale}
        onChange={(excludeOnSale) => set({ excludeOnSale }, "excludeOnSale")}
        isDisabled={isReadOnly}
      />

      {isPicking && (
        <ProductPicker
          title="Products it applies to"
          isAlreadyIn={(p) => items.products.includes(p.id)}
          onConfirm={async (picked) => {
            onNames(Object.fromEntries(picked.map((p) => [p.id, p.title])));
            set({ products: [...items.products, ...picked.map((p) => p.id)] }, "products");
            setIsPicking(false);
          }}
          onClose={() => setIsPicking(false)}
        />
      )}
    </VStack>
  );
}

/** The Applies to card: a product reward's items; nothing to choose for the others. */
export function AppliesToSection({
  draft,
  errors,
  onChange,
  isReadOnly,
  categories,
  names,
  onNames,
  onPickVariants,
}: SectionProps & {
  categories: readonly Option[];
  names: Readonly<Record<string, string>>;
  onNames: (names: Record<string, string>) => void;
  onPickVariants?: () => void;
}) {
  const kind = draft.kind;
  const product = kind.type === "standard" ? kind.product : null;
  if (kind.type !== "standard") return null;

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Applies to
          </Text>
          {!product && (
            <Text type="supporting" color="secondary">
              {kind.order
                ? "An order discount comes off the whole subtotal."
                : "A shipping discount comes off the chosen shipping."}
            </Text>
          )}
        </VStack>
        {product && (
          <ItemSetFields
            items={product.items}
            path="kind.product.items"
            errors={errors}
            onChange={(items, paths) =>
              onChange({ ...draft, kind: { ...kind, product: { ...product, items } } }, paths)
            }
            isReadOnly={isReadOnly}
            categories={categories}
            names={names}
            onNames={onNames}
            onPickVariants={onPickVariants}
          />
        )}
      </VStack>
    </Card>
  );
}
