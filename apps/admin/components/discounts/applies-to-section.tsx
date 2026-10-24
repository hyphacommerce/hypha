"use client";

// Which items a product discount covers: every product, chosen products, variants or
// categories (with their subcategories), and whether items already on sale are left out.

import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Divider } from "@astryxdesign/core/Divider";
import { VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { Text } from "@astryxdesign/core/Text";
import { useState } from "react";

import { ProductPicker } from "@/components/product-picker";
import type { ItemSetDraft, ItemsScope } from "@/lib/discounts/draft";
import type { FieldErrors } from "@/lib/discounts/validate";

import { FieldError, IdTokens, type Option, type SectionProps, fieldStatus } from "./fields";
import { SearchPicker } from "./search-picker";

const SCOPES: readonly ItemsScope[] = ["all", "products", "variants", "categories"];

const scopeOf = (value: string): ItemsScope => SCOPES.find((s) => s === value) ?? "all";

/** An item set's fields, for a reward or either side of a buy X get Y. */
export function ItemSetFields({
  label = "Applies to",
  items,
  path,
  errors,
  onChange,
  isReadOnly,
  categories,
  names,
  onNames,
}: {
  /** The scope's label: "Applies to", "Customer buys", "Customer gets". */
  label?: string;
  items: ItemSetDraft;
  path: string;
  errors: FieldErrors;
  onChange: (items: ItemSetDraft, paths: string[]) => void;
  isReadOnly?: boolean;
  categories: readonly Option[];
  names: Readonly<Record<string, string>>;
  /** Names of products and variants picked here, for the tokens and the summary. */
  onNames: (names: Record<string, string>) => void;
}) {
  const [picking, setPicking] = useState<"products" | "variants" | null>(null);
  const set = (change: Partial<ItemSetDraft>, field: string) =>
    onChange({ ...items, ...change }, [`${path}.${field}`]);

  return (
    <VStack gap={3}>
      <RadioList
        label={label}
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
      <FieldError message={errors[`${path}.scope`]} />

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
              onClick={() => setPicking("products")}
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
          {!isReadOnly && (
            <Button
              label="Choose variants"
              variant="secondary"
              onClick={() => setPicking("variants")}
            />
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

      {picking === "products" && (
        <ProductPicker
          title={`${label}: products`}
          isAlreadyIn={(p) => items.products.includes(p.id)}
          onConfirm={async (picked) => {
            onNames(Object.fromEntries(picked.map((p) => [p.id, p.title])));
            set({ products: [...items.products, ...picked.map((p) => p.id)] }, "products");
            setPicking(null);
          }}
          onClose={() => setPicking(null)}
        />
      )}
      {picking === "variants" && (
        <SearchPicker
          source="variants"
          title={`${label}: variants`}
          chosen={items.variants}
          onConfirm={(picked) => {
            onNames(Object.fromEntries(picked.map((v) => [v.id, v.label])));
            set({ variants: [...items.variants, ...picked.map((v) => v.id)] }, "variants");
            setPicking(null);
          }}
          onClose={() => setPicking(null)}
        />
      )}
    </VStack>
  );
}

/**
 * The Applies to card: a product reward's items, the two sides of a buy X get Y, or the
 * items tiers count; nothing to choose for an order or shipping reward.
 */
export function AppliesToSection({
  draft,
  errors,
  onChange,
  isReadOnly,
  categories,
  names,
  onNames,
}: SectionProps & {
  categories: readonly Option[];
  names: Readonly<Record<string, string>>;
  onNames: (names: Record<string, string>) => void;
}) {
  const kind = draft.kind;
  const shared = { errors, isReadOnly, categories, names, onNames };

  if (kind.type === "buy_x_get_y") {
    return (
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Applies to
            </Text>
            <Text type="supporting" color="secondary">
              The units bought count the most expensive first; the units got are the cheapest left.
              One item may be on both sides: buy 2, get 1 free.
            </Text>
          </VStack>
          <ItemSetFields
            {...shared}
            label="Customer buys"
            items={kind.buys}
            path="kind.buys"
            onChange={(buys, paths) => onChange({ ...draft, kind: { ...kind, buys } }, paths)}
          />
          <Divider />
          <ItemSetFields
            {...shared}
            label="Customer gets"
            items={kind.gets}
            path="kind.gets"
            onChange={(gets, paths) => onChange({ ...draft, kind: { ...kind, gets } }, paths)}
          />
        </VStack>
      </Card>
    );
  }

  if (kind.type === "tiered") {
    return (
      <Card>
        <VStack gap={4}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Applies to
            </Text>
            <Text type="supporting" color="secondary">
              The items the tiers count, and that their discount comes off.
            </Text>
          </VStack>
          <ItemSetFields
            {...shared}
            items={kind.items}
            path="kind.items"
            onChange={(items, paths) => onChange({ ...draft, kind: { ...kind, items } }, paths)}
          />
        </VStack>
      </Card>
    );
  }

  const product = kind.product;
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
            {...shared}
            items={product.items}
            path="kind.product.items"
            onChange={(items, paths) =>
              onChange({ ...draft, kind: { ...kind, product: { ...product, items } } }, paths)
            }
          />
        )}
      </VStack>
    </Card>
  );
}
