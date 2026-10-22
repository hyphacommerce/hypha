"use client";

// The product page's interactive part (the `product-detail` template): the gallery,
// the option pickers, quantity and "Add to cart". Price, availability and image
// follow the choice; nothing is fetched here, since the page passed every variant
// priced in the shopper's country.

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
import { startTransition, useState, useTransition } from "react";

import { useCart } from "@/components/cart/cart-provider";
import { CustomFields } from "@/components/custom-fields";
import { productImageName } from "@/components/product-grid";
import {
  HERO_IMAGE,
  ViewTransition,
  WITHIN_PAGE,
  addTransitionType,
} from "@/components/view-transition";
import { type FieldEntry, withVariant } from "@/lib/custom-fields";
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

/** The product's custom fields and each variant's (by ID), formatted by the page. */
export type ProductFields = { product: FieldEntry[]; variants: Record<string, FieldEntry[]> };

export function ProductForm({
  product,
  digits,
  fields,
}: {
  product: Product;
  digits: number;
  fields: ProductFields;
}) {
  // Start on the first variant that can be bought, else the first.
  const start = product.variants.find((v) => v.available) ?? product.variants[0];
  const cart = useCart();
  const [selections, setSelections] = useState<string[]>(start?.selections ?? []);
  const [wanted, setQuantity] = useState(1);
  const [error, setError] = useState<string>();
  const [pending, startAdding] = useTransition();
  const variant = variantWith(product, selections);
  // How many more can go in the cart: the stock left, less what's in it already (the API
  // refuses more). `null` stock means no limit.
  const inCart = cart.cart?.lines.find((l) => l.variant === variant?.id)?.quantity ?? 0;
  const stock = variant?.available_quantity;
  const room = stock == null ? MAX_QUANTITY : Math.min(MAX_QUANTITY, Math.max(0, stock - inCart));
  const quantity = Math.max(1, Math.min(wanted, room));

  const images = product.media.filter((m) => m.kind === "image");
  // Start on the variant's image, if it has one; picking a variant moves to its image.
  const [shown, setShown] = useState(() =>
    Math.max(
      0,
      images.findIndex((m) => m.asset_id === variant?.media_asset_id),
    ),
  );
  const hero = images[shown] ?? images[0];
  // In a transition, within the page: the hero cross-fades to the image (its boundary).
  const showImage = (index: number) =>
    startTransition(() => {
      addTransitionType(WITHIN_PAGE);
      addTransitionType(HERO_IMAGE);
      setShown(index);
    });

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
    const image = images.findIndex(
      (m) => m.asset_id === variantWith(product, next)?.media_asset_id,
    );
    // In a transition: the hero cross-fades to the new image (its ViewTransition).
    if (image >= 0) showImage(image);
  };

  const add = () => {
    if (!variant) return;
    startAdding(async () => {
      // Opens the cart drawer when it's in.
      setError(await cart.add(variant.id, quantity));
    });
  };

  // The selected variant's values win over the product's.
  const custom = withVariant(fields.product, variant ? fields.variants[variant.id] : undefined);
  const hasDetails = Boolean(
    product.vendor ||
    product.product_type ||
    variant?.title ||
    product.tags.length > 0 ||
    custom.length > 0,
  );

  const label = !variant
    ? "This combination isn't sold"
    : !variant.available
      ? "Sold out"
      : room === 0
        ? "All in stock are in your cart"
        : "Add to cart";

  return (
    <Grid columns={{ minWidth: 340, repeat: "fit" }} gap={8}>
      <VStack gap={3}>
        {/* The card's image travels here from the listing (the shared name), and another
            image cross-fades in when a thumbnail or a variant picks it. */}
        <ViewTransition
          name={productImageName(product.handle)}
          default="none"
          share="shop-image-share"
          update={{ [HERO_IMAGE]: "shop-hero-fade", default: "none" }}
        >
          <div className="shop-rounded">
            <AspectRatio ratio={4 / 5}>
              {hero ? (
                <img
                  className="shop-fill"
                  src={hero.url}
                  alt={hero.alt ?? product.title}
                  // The page's largest paint: fetched first.
                  fetchPriority="high"
                />
              ) : (
                <div />
              )}
            </AspectRatio>
          </div>
        </ViewTransition>
        {images.length > 1 && (
          <Grid columns={5} gap={2}>
            {images.map((m, i) => (
              <AspectRatio key={m.asset_id} ratio={1}>
                <SelectableCard
                  label={m.alt ?? `Image ${i + 1}`}
                  isSelected={i === shown}
                  onChange={() => showImage(i)}
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
              {product.prices_include_tax === false && <Text type="supporting">plus tax</Text>}
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
                  clickAction={() => setQuantity(Math.max(1, quantity - 1))}
                />
                <div style={{ width: 88 }}>
                  <NumberInput
                    label="Quantity"
                    isLabelHidden
                    value={quantity}
                    onChange={(q) => setQuantity(Math.min(room, Math.max(1, q || 1)))}
                    min={1}
                    max={Math.max(1, room)}
                    isDisabled={room === 0}
                    isIntegerOnly
                  />
                </div>
                <Button
                  label="More"
                  variant="ghost"
                  isIconOnly
                  icon={<Icon icon={PlusIcon} size="sm" />}
                  isDisabled={quantity >= room}
                  clickAction={() => setQuantity(Math.min(room, quantity + 1))}
                />
              </HStack>
              {variant?.available && stock != null && stock <= 5 && (
                <Text type="supporting">{stock === 0 ? "None left" : `Only ${stock} left`}</Text>
              )}
            </VStack>
            <Button
              label={label}
              variant="primary"
              size="lg"
              width="100%"
              isLoading={pending}
              isDisabled={!variant?.available || room === 0}
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
                <VStack gap={3}>
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
                  <CustomFields entries={custom} />
                </VStack>
              </Collapsible>
            )}
            {hasDetails && <Divider />}
          </CollapsibleGroup>
        </VStack>
      </div>
    </Grid>
  );
}
