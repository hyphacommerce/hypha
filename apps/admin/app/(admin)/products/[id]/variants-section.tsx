"use client";

import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Selector } from "@astryxdesign/core/Selector";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Thumbnail } from "@astryxdesign/core/Thumbnail";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { moneyInput, parseMoney } from "@/lib/format";

import { addVariant, removeVariant, setVariantMedia, updateVariant } from "../actions";

type Product = AdminSchemas["ProductResponse"];
type Variant = AdminSchemas["VariantResponse"];
type Item = AdminSchemas["ProductMediaResponse"];

/**
 * The variants (design note 0014, section 8): SKU, price, weight and whether it ships
 * (design note 0015) edited in place and saved per row, an image per variant from the product's media, and Add variant for one
 * combination at a time. Nothing generates every combination of the options.
 */
export function VariantsSection({ product }: { product: Product }) {
  const [isAdding, setIsAdding] = useState(false);
  const hasOptions = product.options.length > 0;
  const images = product.media.filter((item) => item.kind === "image");

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Variants
            </Text>
            <Text type="supporting" color="secondary">
              Base prices, before markets and price lists adjust them.
            </Text>
          </VStack>
          <Button
            label="Add variant"
            variant="secondary"
            isDisabled={!hasOptions}
            tooltip={
              hasOptions ? undefined : "Add an option first: a product without options has one variant."
            }
            onClick={() => setIsAdding(true)}
          />
        </HStack>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHeaderCell>Image</TableHeaderCell>
              <TableHeaderCell>Variant</TableHeaderCell>
              <TableHeaderCell>SKU</TableHeaderCell>
              <TableHeaderCell>Price</TableHeaderCell>
              <TableHeaderCell>Weight (g)</TableHeaderCell>
              <TableHeaderCell>Ships</TableHeaderCell>
              <TableHeaderCell>
                <span />
              </TableHeaderCell>
            </TableRow>
          </TableHeader>
          <TableBody>
            {product.variants.map((variant) => (
              <VariantRow
                key={`${variant.id}:${variant.sku}:${variant.price.amount}:${variant.media_asset_id}:${variant.weight_grams}:${variant.requires_shipping}`}
                productId={product.id}
                variant={variant}
                title={variant.title ?? (hasOptions ? "—" : "Default")}
                images={images}
                isOnly={product.variants.length === 1}
              />
            ))}
          </TableBody>
        </Table>
      </VStack>
      {isAdding && <AddVariantDialog product={product} onClose={() => setIsAdding(false)} />}
    </Card>
  );
}

function VariantRow({
  productId,
  variant,
  title,
  images,
  isOnly,
}: {
  productId: string;
  variant: Variant;
  title: string;
  images: Item[];
  isOnly: boolean;
}) {
  const currencies = useCurrencies();
  const savedSku = variant.sku ?? "";
  const savedPrice = moneyInput(variant.price, currencies);
  const savedWeight = variant.weight_grams == null ? "" : String(variant.weight_grams);
  const [sku, setSku] = useState(savedSku);
  const [price, setPrice] = useState(savedPrice);
  const [weight, setWeight] = useState(savedWeight);
  const [ships, setShips] = useState(variant.requires_shipping);
  const [error, setError] = useState<string>();
  const [isConfirmingDelete, setIsConfirmingDelete] = useState(false);
  const [isPickingImage, setIsPickingImage] = useState(false);
  const [isPending, startTransition] = useTransition();

  // Compared as the API stores them (trimmed, in minor units): "A " or "45" saved as
  // "A" and "45.00" would otherwise look unsaved forever.
  const isDirty =
    sku.trim() !== savedSku ||
    parseMoney(price, variant.price.currency, currencies) !== variant.price.amount ||
    weight.trim() !== savedWeight ||
    ships !== variant.requires_shipping;
  const image = images.find((item) => item.asset_id === variant.media_asset_id);
  const save = () => {
    if (!isDirty) return;
    startTransition(async () => {
      const result = await updateVariant(productId, variant.id, {
        sku,
        price,
        currency: variant.price.currency,
        weight,
        requiresShipping: ships,
      });
      setError(result.error);
      if (!result.error) {
        setSku(sku.trim());
        setWeight(weight.trim());
      }
    });
  };

  return (
    <TableRow>
      <TableCell>
        <Thumbnail
          src={image?.thumbnail_url ?? image?.url}
          alt={image?.alt ?? ""}
          label={image ? `Change the image of ${title}` : `Choose an image for ${title}`}
          onClick={() => setIsPickingImage(true)}
        />
        {isPickingImage && (
          <VariantImageDialog
            productId={productId}
            variant={variant}
            title={title}
            images={images}
            onClose={() => setIsPickingImage(false)}
          />
        )}
      </TableCell>
      <TableCell>
        <Text>{title}</Text>
      </TableCell>
      <TableCell>
        <TextInput
          label={`SKU of ${title}`}
          isLabelHidden
          size="sm"
          value={sku}
          onChange={setSku}
          onEnter={save}
          placeholder="—"
          width={130}
        />
      </TableCell>
      <TableCell>
        <TextInput
          label={`Price of ${title} in ${variant.price.currency}`}
          isLabelHidden
          size="sm"
          value={price}
          onChange={setPrice}
          onEnter={save}
          width={110}
          status={error ? { type: "error", message: error } : undefined}
          statusVariant="tooltip"
        />
      </TableCell>
      <TableCell>
        <TextInput
          label={`Weight of ${title} in grams`}
          isLabelHidden
          size="sm"
          value={weight}
          onChange={setWeight}
          onEnter={save}
          placeholder="—"
          width={80}
        />
      </TableCell>
      <TableCell>
        <CheckboxInput
          label={`${title} is shipped`}
          isLabelHidden
          size="sm"
          value={ships}
          onChange={setShips}
        />
      </TableCell>
      <TableCell>
        <HStack gap={1} hAlign="end" vAlign="center">
          {isConfirmingDelete ? (
            <>
              <Button
                label="Cancel"
                size="sm"
                variant="ghost"
                onClick={() => setIsConfirmingDelete(false)}
              />
              <Button
                label="Delete variant"
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    const result = await removeVariant(productId, variant.id);
                    setError(result.error);
                    setIsConfirmingDelete(false);
                  })
                }
              />
            </>
          ) : (
            <>
              {isDirty && (
                <Button label="Save" size="sm" variant="primary" isLoading={isPending} onClick={save} />
              )}
              <Button
                label="Delete"
                size="sm"
                variant="ghost"
                isDisabled={isOnly}
                tooltip={isOnly ? "A product keeps at least one variant." : undefined}
                onClick={() => setIsConfirmingDelete(true)}
              />
            </>
          )}
        </HStack>
      </TableCell>
    </TableRow>
  );
}

/** One of the product's images for a variant, or none. */
function VariantImageDialog({
  productId,
  variant,
  title,
  images,
  onClose,
}: {
  productId: string;
  variant: Variant;
  title: string;
  images: Item[];
  onClose: () => void;
}) {
  const [chosen, setChosen] = useState<string | null>(variant.media_asset_id ?? null);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560}>
      <DialogHeader
        title={`Image for ${title}`}
        subtitle="One of the product's images."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {images.length === 0 ? (
          <Text color="secondary">Add images to the product first, in Media above.</Text>
        ) : (
          <Grid columns={{ minWidth: 110 }} gap={2}>
            {images.map((item) => (
              <SelectableCard
                key={item.asset_id}
                label={item.alt ?? "Product image"}
                isSelected={chosen === item.asset_id}
                onChange={(isSelected) => setChosen(isSelected ? item.asset_id : null)}
                padding={0}
              >
                <AspectRatio ratio={1} fit="cover">
                  <img src={item.thumbnail_url ?? item.url} alt={item.alt ?? ""} />
                </AspectRatio>
              </SelectableCard>
            ))}
          </Grid>
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={chosen ? "Use this image" : "Show no image"}
            variant="primary"
            isLoading={isPending}
            isDisabled={chosen === (variant.media_asset_id ?? null)}
            onClick={() =>
              startTransition(async () => {
                const result = await setVariantMedia(productId, variant.id, chosen);
                if (result.error) setError(result.error);
                else onClose();
              })
            }
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** One combination: a value per option, a SKU and a price. */
function AddVariantDialog({ product, onClose }: { product: Product; onClose: () => void }) {
  const [selections, setSelections] = useState<Record<string, string>>({});
  const [sku, setSku] = useState("");
  const [price, setPrice] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const currency = product.variants[0]?.price.currency ?? "EUR";

  const isComplete =
    product.options.every((option) => selections[option.id]) && price.trim() !== "";
  const submit = () => {
    if (!isComplete) return;
    startTransition(async () => {
      const result = await addVariant(product.id, {
        sku,
        price,
        currency,
        selections: product.options.map((option) => selections[option.id]),
      });
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader title="Add variant" onOpenChange={(isOpen) => !isOpen && onClose()} />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {product.options.map((option) => (
          <Selector
            key={option.id}
            label={option.name}
            placeholder={`Choose a ${option.name.toLowerCase()}`}
            options={option.values.map((value) => ({ value: value.id, label: value.label }))}
            value={selections[option.id]}
            onChange={(value) => setSelections((before) => ({ ...before, [option.id]: value }))}
          />
        ))}
        <TextInput label="SKU" isOptional value={sku} onChange={setSku} onEnter={submit} />
        <TextInput
          label={`Price (${currency})`}
          placeholder="18.00"
          value={price}
          onChange={setPrice}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Add variant"
            variant="primary"
            isDisabled={!isComplete}
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
