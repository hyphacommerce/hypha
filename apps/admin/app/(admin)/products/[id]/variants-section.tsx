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

import { setVariantMedia } from "../actions";
import {
  type Draft,
  type VariantDraft,
  kept,
  newKey,
  valuesOf,
  variantTitle,
  withTypedValues,
} from "./draft";

type Product = AdminSchemas["ProductResponse"];
type Item = AdminSchemas["ProductMediaResponse"];

/**
 * The variants (design note 0014, section 8): SKU, price, weight and whether it ships
 * (design note 0015), part of the editor's draft and saved as one list (design note
 * 0021, Q1). A variant's image is a separate decision and saves at once, for saved
 * variants. Add variant adds one combination; "Create all variants", in Options, the
 * rest.
 */
export function VariantsSection({
  product,
  draft,
  currency,
  onChange,
}: {
  /** As saved: the variants' images and the product's media. */
  product: Product;
  draft: Draft;
  /** The base prices' currency. */
  currency: string;
  onChange: (change: Partial<Draft>) => void;
}) {
  const [isAdding, setIsAdding] = useState(false);
  const hasOptions = draft.options.length > 0;
  const images = product.media.filter((item) => item.kind === "image");
  const rows = kept(draft);

  const update = (key: string, change: Partial<VariantDraft>) =>
    onChange({ variants: draft.variants.map((v) => (v.key === key ? { ...v, ...change } : v)) });

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Variants
            </Text>
            <Text type="supporting" color="secondary">
              Base prices in {currency}, before markets and price lists adjust them.
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
            {rows.map((variant) => (
              <VariantRow
                key={variant.key}
                productId={product.id}
                variant={variant}
                saved={product.variants.find((v) => v.id === variant.id)}
                title={variantTitle(draft, variant) ?? "Default"}
                currency={currency}
                images={images}
                isOnly={rows.length === 1}
                onChange={(change) => update(variant.key, change)}
                onRemove={() =>
                  onChange({ variants: draft.variants.filter((v) => v.key !== variant.key) })
                }
              />
            ))}
          </TableBody>
        </Table>
      </VStack>
      {isAdding && (
        <AddVariantDialog
          draft={draft}
          currency={currency}
          onAdd={(change) => {
            onChange(change);
            setIsAdding(false);
          }}
          onClose={() => setIsAdding(false)}
        />
      )}
    </Card>
  );
}

function VariantRow({
  productId,
  variant,
  saved,
  title,
  currency,
  images,
  isOnly,
  onChange,
  onRemove,
}: {
  productId: string;
  variant: VariantDraft;
  /** The variant as saved; `undefined` for one added in this draft. */
  saved?: AdminSchemas["VariantResponse"];
  title: string;
  currency: string;
  images: Item[];
  isOnly: boolean;
  onChange: (change: Partial<VariantDraft>) => void;
  onRemove: () => void;
}) {
  const [isPickingImage, setIsPickingImage] = useState(false);
  const image = images.find((item) => item.asset_id === saved?.media_asset_id);

  return (
    <TableRow>
      <TableCell>
        <Thumbnail
          src={image?.thumbnail_url ?? image?.url}
          alt={image?.alt ?? ""}
          label={
            saved
              ? image
                ? `Change the image of ${title}`
                : `Choose an image for ${title}`
              : `Save ${title} before choosing its image`
          }
          onClick={saved ? () => setIsPickingImage(true) : undefined}
        />
        {isPickingImage && saved && (
          <VariantImageDialog
            productId={productId}
            variant={saved}
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
          value={variant.sku}
          onChange={(sku) => onChange({ sku })}
          placeholder="—"
          width={130}
        />
      </TableCell>
      <TableCell>
        <TextInput
          label={`Price of ${title} in ${currency}`}
          isLabelHidden
          size="sm"
          value={variant.price}
          onChange={(price) => onChange({ price })}
          width={110}
        />
      </TableCell>
      <TableCell>
        <TextInput
          label={`Weight of ${title} in grams`}
          isLabelHidden
          size="sm"
          value={variant.weight}
          onChange={(weight) => onChange({ weight })}
          placeholder="—"
          width={80}
        />
      </TableCell>
      <TableCell>
        <CheckboxInput
          label={`${title} is shipped`}
          isLabelHidden
          size="sm"
          value={variant.ships}
          onChange={(ships) => onChange({ ships })}
        />
      </TableCell>
      <TableCell>
        <HStack gap={1} hAlign="end" vAlign="center">
          <Button
            label="Remove"
            size="sm"
            variant="ghost"
            isDisabled={isOnly}
            tooltip={isOnly ? "A product keeps at least one variant." : undefined}
            onClick={onRemove}
          />
        </HStack>
      </TableCell>
    </TableRow>
  );
}

/** One of the product's images for a saved variant, or none; saves at once. */
function VariantImageDialog({
  productId,
  variant,
  title,
  images,
  onClose,
}: {
  productId: string;
  variant: AdminSchemas["VariantResponse"];
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

/** One combination: a value per option (values added in the draft included), a SKU and
 * a price, added to the draft. */
function AddVariantDialog({
  draft: current,
  currency,
  onAdd,
  onClose,
}: {
  draft: Draft;
  currency: string;
  onAdd: (change: Partial<Draft>) => void;
  onClose: () => void;
}) {
  // A value typed but not yet entered can be chosen too.
  const [draft] = useState(() => withTypedValues(current));
  const [selections, setSelections] = useState<Record<string, string>>({});
  const [sku, setSku] = useState("");
  const [price, setPrice] = useState("");

  const isComplete =
    draft.options.every((option) => selections[option.key]) && price.trim() !== "";
  const submit = () => {
    if (!isComplete) return;
    onAdd({
      options: draft.options,
      variants: [
        ...draft.variants,
        { key: newKey(), sku, price, weight: "", ships: true, selections },
      ],
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title="Add variant"
        subtitle="It's saved with the rest of your changes."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {draft.options.map((option) => (
          <Selector
            key={option.key}
            label={option.name || "Option"}
            placeholder={`Choose a ${(option.name || "value").toLowerCase()}`}
            options={valuesOf(option).map((value) => ({ value: value.key, label: value.label }))}
            value={selections[option.key]}
            onChange={(value) => setSelections((before) => ({ ...before, [option.key]: value }))}
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
          <Button label="Add variant" variant="primary" isDisabled={!isComplete} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
