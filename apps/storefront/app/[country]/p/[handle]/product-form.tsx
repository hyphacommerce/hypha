"use client";

// The product page's interactive part (design note 0024, §7; the `product-detail`
// template): the gallery, the option pickers, quantity and "Add to cart". Price,
// availability and image follow the choice; nothing is fetched here, since the page
// passed every variant priced in the shopper's country.

import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Collapsible, CollapsibleGroup } from "@astryxdesign/core/Collapsible";
import { Divider } from "@astryxdesign/core/Divider";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { Icon } from "@astryxdesign/core/Icon";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { MinusIcon, PlusIcon } from "@heroicons/react/24/outline";
import type { StorefrontSchemas } from "@hypha/sdk/storefront";
import { useState, useTransition } from "react";

import { addToCart } from "@/app/[country]/actions";
import { formatMoney } from "@/lib/money";

type Product = StorefrontSchemas["StorefrontProductResponse"];
type Variant = StorefrontSchemas["StorefrontVariantResponse"];

/** The most a shopper adds at once; the cart takes more. */
const MAX_QUANTITY = 99;

/** The variant with exactly these selections. */
function variantWith(product: Product, selections: string[]): Variant | undefined {
  return product.variants.find((v) => v.selections.every((s, i) => s === selections[i]));
}

/** Whether a variant that can be bought has `value` for option `index`, agreeing with
 * the choices made before it: the options are picked in order, so a value is only
 * offered where it leads somewhere. */
function offered(product: Product, selections: string[], index: number, value: string): boolean {
  return product.variants.some(
    (v) =>
      v.available &&
      v.selections[index] === value &&
      v.selections.slice(0, index).every((s, i) => s === selections[i]),
  );
}

export function ProductForm({
  product,
  country,
  digits,
}: {
  product: Product;
  country: string;
  digits: number;
}) {
  // Start on the first variant that can be bought, else the first.
  const start = product.variants.find((v) => v.available) ?? product.variants[0];
  const [selections, setSelections] = useState<string[]>(start?.selections ?? []);
  const [quantity, setQuantity] = useState(1);
  const [error, setError] = useState<string>();
  const [pending, startAdding] = useTransition();
  const variant = variantWith(product, selections);

  const images = product.media.filter((m) => m.kind === "image");
  const variantImage = images.findIndex((m) => m.asset_id === variant?.media_asset_id);
  const [shown, setShown] = useState(Math.max(0, variantImage));
  const hero = images[shown] ?? images[0];

  const pick = (index: number, value: string) => {
    let next = [...selections];
    next[index] = value;
    // Keep the later choices if that combination can be bought, else move to the first
    // one that can with the choices so far.
    if (!variantWith(product, next)?.available) {
      const fit = product.variants.find(
        (v) => v.available && v.selections.slice(0, index + 1).every((s, i) => s === next[i]),
      );
      if (fit) next = [...fit.selections];
    }
    setSelections(next);
    setError(undefined);
    const image = images.findIndex((m) => m.asset_id === variantWith(product, next)?.media_asset_id);
    if (image >= 0) setShown(image);
  };

  const add = () => {
    if (!variant) return;
    startAdding(async () => {
      const result = await addToCart({ country, variant: variant.id, quantity });
      if (result) setError(result.error);
    });
  };

  const hasDetails = Boolean(
    product.vendor || product.product_type || variant?.title || product.tags.length > 0,
  );

  const label = !variant
    ? "This combination isn't sold"
    : variant.available
      ? "Add to cart"
      : "Sold out";

  return (
    <div className="shop-split">
      <VStack gap={3}>
        <div className="shop-rounded">
          <AspectRatio ratio={4 / 5}>
            {hero ? (
              <img className="shop-fill" src={hero.url} alt={hero.alt ?? product.title} />
            ) : (
              <div />
            )}
          </AspectRatio>
        </div>
        {images.length > 1 && (
          <Grid columns={5} gap={2}>
            {images.map((m, i) => (
              <AspectRatio key={m.asset_id} ratio={1}>
                <SelectableCard
                  label={m.alt ?? `Image ${i + 1}`}
                  isSelected={i === shown}
                  onChange={() => setShown(i)}
                  variant="transparent"
                  padding={0}
                  width="100%"
                  height="100%"
                >
                  <img className="shop-fill" src={m.thumbnail_url ?? m.url} alt="" />
                </SelectableCard>
              </AspectRatio>
            ))}
          </Grid>
        )}
      </VStack>

      <div className="shop-sticky">
        <VStack gap={6}>
          <VStack gap={2}>
            {product.vendor && (
              <Text type="label" color="secondary">
                {product.vendor}
              </Text>
            )}
            <Heading level={1} type="display-2">
              {product.title}
            </Heading>
            <HStack gap={2} vAlign="center">
              {variant?.price ? (
                <Text type="large" weight="semibold">
                  {formatMoney(variant.price, digits)}
                </Text>
              ) : null}
              {product.prices_include_tax === false && (
                <Text type="supporting">plus tax</Text>
              )}
              {variant && !variant.available && <Badge label="Sold out" variant="neutral" />}
            </HStack>
          </VStack>

          {product.options.map((option, index) => (
            <VStack key={option.id} gap={2}>
              <Text type="label">
                {option.name}
                {": "}
                <Text type="label" color="secondary">
                  {option.values.find((v) => v.id === selections[index])?.label ?? "Choose"}
                </Text>
              </Text>
              <HStack gap={2} wrap="wrap">
                {option.values.map((value) => {
                  const enabled = offered(product, selections, index, value.id);
                  return (
                    <SelectableCard
                      key={value.id}
                      label={enabled ? value.label : `${value.label} (unavailable)`}
                      isSelected={selections[index] === value.id}
                      isDisabled={!enabled}
                      onChange={() => pick(index, value.id)}
                      padding={2}
                    >
                      <Text
                        type="body"
                        weight="medium"
                        color={enabled ? "primary" : "disabled"}
                        hasStrikethrough={!enabled}
                      >
                        {value.label}
                      </Text>
                    </SelectableCard>
                  );
                })}
              </HStack>
            </VStack>
          ))}

          <VStack gap={3}>
            <VStack gap={2}>
              <Text type="label">Quantity</Text>
              <HStack gap={1} vAlign="center">
                <Button
                  label="Fewer"
                  variant="ghost"
                  isIconOnly
                  icon={<Icon icon={MinusIcon} size="sm" />}
                  isDisabled={quantity <= 1}
                  clickAction={() => setQuantity((q) => Math.max(1, q - 1))}
                />
                <div style={{ width: 88 }}>
                  <NumberInput
                    label="Quantity"
                    isLabelHidden
                    value={quantity}
                    onChange={(q) => setQuantity(Math.min(MAX_QUANTITY, Math.max(1, q || 1)))}
                    min={1}
                    max={MAX_QUANTITY}
                    isIntegerOnly
                  />
                </div>
                <Button
                  label="More"
                  variant="ghost"
                  isIconOnly
                  icon={<Icon icon={PlusIcon} size="sm" />}
                  isDisabled={quantity >= MAX_QUANTITY}
                  clickAction={() => setQuantity((q) => Math.min(MAX_QUANTITY, q + 1))}
                />
              </HStack>
            </VStack>
            <Button
              label={label}
              variant="primary"
              size="lg"
              width="100%"
              isLoading={pending}
              isDisabled={!variant?.available}
              clickAction={add}
            />
            {error && <Banner status="error" title={error} />}
          </VStack>

          <CollapsibleGroup type="multiple" defaultValue={["description"]}>
            <Divider />
            {product.description && (
              <>
                <Collapsible value="description" trigger={<Heading level={3}>Description</Heading>}>
                  <div
                    className="shop-prose"
                    // Sanitised by the API (scripts, styles and event attributes removed).
                    dangerouslySetInnerHTML={{ __html: product.description }}
                  />
                </Collapsible>
                <Divider />
              </>
            )}
            {hasDetails && (
            <Collapsible value="details" trigger={<Heading level={3}>Details</Heading>}>
              <VStack gap={1}>
                {product.vendor && <Text>Brand: {product.vendor}</Text>}
                {product.product_type && <Text>Type: {product.product_type}</Text>}
                {variant?.title && <Text>Variant: {variant.title}</Text>}
                {product.tags.length > 0 && (
                  <HStack gap={1} wrap="wrap">
                    {product.tags.map((t) => (
                      <Badge key={t} label={t} variant="neutral" />
                    ))}
                  </HStack>
                )}
              </VStack>
            </Collapsible>
            )}
            {hasDetails && <Divider />}
          </CollapsibleGroup>
        </VStack>
      </div>
    </div>
  );
}
