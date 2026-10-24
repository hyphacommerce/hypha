"use client";

import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Grid } from "@astryxdesign/core/Grid";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { SelectableCard } from "@astryxdesign/core/SelectableCard";
import { Selector } from "@astryxdesign/core/Selector";
import { Table, type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Thumbnail } from "@astryxdesign/core/Thumbnail";
import { MagnifyingGlassIcon, PlusIcon, TrashIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useEffect, useState, useTransition } from "react";

import { VariantFieldsCell } from "@/components/custom-fields/variant-fields";
import type { Definition } from "@/lib/custom-fields";

import {
  type Stock,
  type StockChange,
  saveVariantStock,
  setVariantMedia,
  variantStock,
} from "../actions";
import {
  type Draft,
  type VariantDraft,
  kept,
  missingCombinations,
  newKey,
  valuesOf,
  variantTitle,
  withAllCombinations,
  withTypedValues,
} from "./draft";

type Product = AdminSchemas["ProductResponse"];
type Item = AdminSchemas["ProductMediaResponse"];
type Location = AdminSchemas["LocationResponse"];

/** A row of the variants table. */
interface Row extends Record<string, unknown> {
  key: string;
  variant: VariantDraft;
}

/** Above this many variants the card offers a filter. */
const FILTER_FROM = 8;

/**
 * The variants: SKU, price, weight and whether it ships, part of the editor's draft
 * and saved as one list. Selected variants can be priced, weighed or removed together.
 * A variant's image and stock are separate decisions and save at once, for saved
 * variants.
 */
export function VariantsSection({
  product,
  draft,
  currency,
  stock,
  locations,
  variantFields,
  onChange,
}: {
  /** As saved: the variants' images and the product's media. */
  product: Product;
  draft: Draft;
  /** The base prices' currency. */
  currency: string;
  /** Saved variants' stock by variant ID; `undefined` when it couldn't be read (no
   * Stock column then). A variant missing from it is read when its stock is opened. */
  stock?: Record<string, Stock>;
  locations: Location[];
  /** The variants' custom field definitions; none: no Fields column. */
  variantFields?: { definitions: Definition[]; scopes: readonly string[] | null };
  onChange: (change: Partial<Draft>) => void;
}) {
  const [isAdding, setIsAdding] = useState(false);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // A message belongs to the variants it was about: a save or a discard replaces them,
  // and the message goes with the old ones.
  const [message, setMessage] = useState<{
    status: "error" | "success";
    text: string;
    variants: VariantDraft[];
  }>();
  const hasOptions = draft.options.length > 0;
  const images = product.media.filter((item) => item.kind === "image");
  const rows = kept(draft);
  const missing = missingCombinations(draft);
  const title = (v: VariantDraft) => variantTitle(draft, v) ?? "Default";
  const needle = filter.trim().toLowerCase();
  const shown = needle
    ? rows.filter((v) => `${title(v)} ${v.sku}`.toLowerCase().includes(needle))
    : rows;
  // Selected rows still in the draft and shown.
  const chosen = shown.filter((v) => selected.has(v.key));

  const setVariants = (variants: VariantDraft[]) => {
    onChange({ variants });
    setMessage(undefined);
  };
  const update = (keys: Set<string>, change: Partial<VariantDraft>) =>
    setVariants(draft.variants.map((v) => (keys.has(v.key) ? { ...v, ...change } : v)));
  const select = (keys: string[], isSelected: boolean) =>
    setSelected((before) => {
      const next = new Set(before);
      for (const key of keys) {
        if (isSelected) next.add(key);
        else next.delete(key);
      }
      return next;
    });
  const createMissing = () => {
    const result = withAllCombinations(draft);
    if ("error" in result) {
      setMessage({ status: "error", text: result.error, variants: draft.variants });
      return;
    }
    const added = result.draft.variants.length - draft.variants.length;
    onChange({ options: result.draft.options, variants: result.draft.variants });
    setMessage({
      variants: result.draft.variants,
      status: "success",
      text: `Added ${added === 1 ? "a variant" : `${added} variants`} priced like the first: check their prices, then save.`,
    });
  };

  const isSelectable = rows.length > 1;
  const columns: TableColumn<Row>[] = [
    ...(isSelectable
      ? [
          {
            key: "select",
            header: (
              <CheckboxInput
                label="Select all shown variants"
                isLabelHidden
                size="sm"
                value={
                  chosen.length === 0
                    ? false
                    : chosen.length === shown.length
                      ? true
                      : "indeterminate"
                }
                onChange={(isSelected: boolean) =>
                  select(
                    shown.map((v) => v.key),
                    isSelected && chosen.length < shown.length,
                  )
                }
              />
            ),
            width: pixel(44),
            resizable: false,
            renderCell: ({ variant }: Row) => (
              <CheckboxInput
                label={`Select ${title(variant)}`}
                isLabelHidden
                size="sm"
                value={selected.has(variant.key)}
                onChange={(isSelected) => select([variant.key], isSelected)}
              />
            ),
          },
        ]
      : []),
    {
      key: "variant",
      header: "Variant",
      width: proportional(2, { minWidth: 120 }),
      renderCell: ({ variant }) => (
        <HStack gap={2} vAlign="center">
          <VariantImage
            productId={product.id}
            saved={product.variants.find((v) => v.id === variant.id)}
            title={title(variant)}
            images={images}
          />
          <Text>{title(variant)}</Text>
        </HStack>
      ),
    },
    {
      key: "sku",
      header: "SKU",
      width: proportional(1.5, { minWidth: 96 }),
      renderCell: ({ variant }) => (
        <TextInput
          label={`SKU of ${title(variant)}`}
          isLabelHidden
          size="sm"
          value={variant.sku}
          onChange={(sku) => update(new Set([variant.key]), { sku })}
          placeholder="—"
          width="100%"
        />
      ),
    },
    {
      key: "price",
      header: `Price (${currency})`,
      width: proportional(1, { minWidth: 84 }),
      renderCell: ({ variant }) => (
        <TextInput
          label={`Price of ${title(variant)} in ${currency}`}
          isLabelHidden
          size="sm"
          value={variant.price}
          onChange={(price) => update(new Set([variant.key]), { price })}
          width="100%"
        />
      ),
    },
    {
      key: "compare_at",
      header: `Compare-at (${currency})`,
      width: proportional(1, { minWidth: 84 }),
      renderCell: ({ variant }) => (
        <TextInput
          label={`Compare-at price of ${title(variant)} in ${currency}`}
          isLabelHidden
          size="sm"
          value={variant.compareAt}
          onChange={(compareAt) => update(new Set([variant.key]), { compareAt })}
          placeholder="—"
          width="100%"
        />
      ),
    },
    {
      key: "weight",
      header: "Weight (g)",
      width: proportional(1, { minWidth: 72 }),
      renderCell: ({ variant }) => (
        <TextInput
          label={`Weight of ${title(variant)} in grams`}
          isLabelHidden
          size="sm"
          value={variant.weight}
          onChange={(weight) => update(new Set([variant.key]), { weight })}
          placeholder="—"
          width="100%"
        />
      ),
    },
    {
      key: "ships",
      header: "Ships",
      width: pixel(64),
      resizable: false,
      align: "center",
      renderCell: ({ variant }) => (
        <CheckboxInput
          label={`${title(variant)} is shipped`}
          isLabelHidden
          size="sm"
          value={variant.ships}
          onChange={(ships) => update(new Set([variant.key]), { ships })}
        />
      ),
    },
    ...(stock
      ? [
          {
            key: "stock",
            header: "Stock",
            width: pixel(112),
            resizable: false,
            renderCell: ({ variant }: Row) => {
              const saved = product.variants.find((v) => v.id === variant.id);
              return saved ? (
                <StockCell
                  productId={product.id}
                  variantId={saved.id}
                  title={title(variant)}
                  stock={stock[saved.id]}
                  locations={locations}
                />
              ) : (
                <Text type="supporting" color="secondary">
                  After saving
                </Text>
              );
            },
          },
        ]
      : []),
    ...(variantFields
      ? [
          {
            key: "fields",
            header: "Fields",
            width: pixel(96),
            resizable: false,
            renderCell: ({ variant }: Row) =>
              variant.id ? (
                <VariantFieldsCell
                  variantId={variant.id}
                  title={title(variant)}
                  currency={currency}
                  definitions={variantFields.definitions}
                  scopes={variantFields.scopes}
                />
              ) : (
                <Text type="supporting" color="secondary">
                  After saving
                </Text>
              ),
          },
        ]
      : []),
    {
      key: "remove",
      header: <span className="visually-hidden">Remove</span>,
      width: pixel(52),
      resizable: false,
      align: "end",
      renderCell: ({ variant }) => (
        <IconButton
          label={`Remove ${title(variant)}`}
          tooltip={rows.length === 1 ? "A product keeps at least one variant." : "Remove variant"}
          size="sm"
          variant="ghost"
          icon={<TrashIcon width={16} />}
          isDisabled={rows.length === 1}
          onClick={() => setVariants(draft.variants.filter((v) => v.key !== variant.key))}
        />
      ),
    },
  ];

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Variants{rows.length > 1 ? ` (${rows.length})` : ""}
            </Text>
            <Text type="supporting" color="secondary">
              Base prices in {currency}, before markets and price lists adjust them. A compare-at
              price shows struck through beside the price, when it&apos;s higher.
            </Text>
          </VStack>
          {hasOptions && (
            <HStack gap={2}>
              {missing !== 0 && (
                <Button
                  label={
                    missing === null
                      ? "Create all variants"
                      : `Create ${missing === 1 ? "the missing variant" : `${missing} missing variants`}`
                  }
                  variant="primary"
                  size="sm"
                  tooltip="A variant for every combination of values that doesn't have one."
                  onClick={createMissing}
                />
              )}
              <Button
                label="Add variant"
                variant="secondary"
                size="sm"
                icon={<PlusIcon width={16} />}
                onClick={() => setIsAdding(true)}
              />
            </HStack>
          )}
        </HStack>

        {message && message.variants === draft.variants && (
          <Banner status={message.status} title={message.text} />
        )}

        {rows.length > FILTER_FROM && (
          <TextInput
            label="Filter variants"
            isLabelHidden
            placeholder="Filter by value or SKU"
            startIcon={<MagnifyingGlassIcon width={16} />}
            size="sm"
            value={filter}
            onChange={setFilter}
            hasClear
            width={280}
          />
        )}

        {chosen.length > 0 && (
          <BulkBar
            count={chosen.length}
            currency={currency}
            canRemove={chosen.length < rows.length}
            onSet={(change) => update(new Set(chosen.map((v) => v.key)), change)}
            onRemove={() => {
              const keys = new Set(chosen.map((v) => v.key));
              setVariants(draft.variants.filter((v) => !keys.has(v.key)));
              select([...keys], false);
            }}
            onClear={() => setSelected(new Set())}
          />
        )}

        <Table
          data={shown.map((variant) => ({ key: variant.key, variant }))}
          idKey="key"
          columns={columns}
        />
        {shown.length === 0 && <Text color="secondary">No variant matches “{filter.trim()}”.</Text>}
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

/** What to do with the selected variants: one price or weight for all, or remove them. */
function BulkBar({
  count,
  currency,
  canRemove,
  onSet,
  onRemove,
  onClear,
}: {
  count: number;
  currency: string;
  canRemove: boolean;
  onSet: (change: Partial<VariantDraft>) => void;
  onRemove: () => void;
  onClear: () => void;
}) {
  const [price, setPrice] = useState("");
  const [weight, setWeight] = useState("");

  return (
    <div
      style={{
        padding: "8px 12px",
        borderRadius: 8,
        background: "var(--color-background-muted)",
      }}
    >
      <HStack gap={3} vAlign="center" wrap="wrap">
        <Text weight="medium">{count === 1 ? "1 selected" : `${count} selected`}</Text>
        <HStack gap={1} vAlign="center">
          <TextInput
            label={`Price for the selected variants in ${currency}`}
            isLabelHidden
            size="sm"
            placeholder={`Price (${currency})`}
            value={price}
            onChange={setPrice}
            onEnter={() => price.trim() && onSet({ price })}
            width={130}
          />
          <Button
            label="Set price"
            size="sm"
            variant="secondary"
            isDisabled={price.trim() === ""}
            onClick={() => onSet({ price })}
          />
        </HStack>
        <HStack gap={1} vAlign="center">
          <TextInput
            label="Weight for the selected variants in grams"
            isLabelHidden
            size="sm"
            placeholder="Weight (g)"
            value={weight}
            onChange={setWeight}
            onEnter={() => onSet({ weight })}
            width={110}
          />
          <Button
            label="Set weight"
            size="sm"
            variant="secondary"
            onClick={() => onSet({ weight })}
          />
        </HStack>
        <Button
          label="Remove"
          size="sm"
          variant="ghost"
          icon={<TrashIcon width={16} />}
          isDisabled={!canRemove}
          tooltip={canRemove ? undefined : "A product keeps at least one variant."}
          onClick={onRemove}
        />
        <Button label="Clear selection" size="sm" variant="ghost" onClick={onClear} />
      </HStack>
    </div>
  );
}

/** A saved variant's image, chosen from the product's at once; a placeholder before
 * the variant is saved. */
function VariantImage({
  productId,
  saved,
  title,
  images,
}: {
  productId: string;
  /** The variant as saved; `undefined` for one added in this draft. */
  saved?: AdminSchemas["VariantResponse"];
  title: string;
  images: Item[];
}) {
  const [isPicking, setIsPicking] = useState(false);
  const image = images.find((item) => item.asset_id === saved?.media_asset_id);
  return (
    <>
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
        onClick={saved ? () => setIsPicking(true) : undefined}
        className="variant-thumbnail"
      />
      {isPicking && saved && (
        <VariantImageDialog
          productId={productId}
          variant={saved}
          title={title}
          images={images}
          onClose={() => setIsPicking(false)}
        />
      )}
    </>
  );
}

/** A saved variant's stock at a glance; opens the dialog that changes it. */
function StockCell({
  productId,
  variantId,
  title,
  stock,
  locations,
}: {
  productId: string;
  variantId: string;
  title: string;
  /** `undefined` when it wasn't read with the page (it's read when opened). */
  stock?: Stock;
  locations: Location[];
}) {
  const [isEditing, setIsEditing] = useState(false);
  return (
    <>
      <Button
        label={stockLabel(stock)}
        size="sm"
        variant="ghost"
        tooltip={`Stock of ${title}`}
        onClick={() => setIsEditing(true)}
      />
      {isEditing && (
        <StockDialog
          productId={productId}
          variantId={variantId}
          title={title}
          initial={stock}
          locations={locations}
          onClose={() => setIsEditing(false)}
        />
      )}
    </>
  );
}

/** "12 in stock", "Untracked", or "Stock" when it hasn't been read. */
function stockLabel(stock?: Stock): string {
  if (!stock) return "Stock";
  if (stock.id == null || !stock.tracked) return "Untracked";
  const available = stock.levels.reduce((n, level) => n + level.available, 0);
  return `${available} in stock`;
}

/**
 * A saved variant's stock, saved at once: whether it's tracked, whether it sells when
 * none is left, and the count on hand at each location. Reads it fresh on open.
 */
function StockDialog({
  productId,
  variantId,
  title,
  initial,
  locations,
  onClose,
}: {
  productId: string;
  variantId: string;
  title: string;
  initial?: Stock;
  locations: Location[];
  onClose: () => void;
}) {
  const [stock, setStock] = useState<Stock | undefined>(initial);
  const [form, setForm] = useState(() => formOf(initial, locations));
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const [isLoading, startLoading] = useTransition();

  // Read it fresh on open: a sale may have moved it since the page loaded.
  useEffect(() => {
    startLoading(async () => {
      const result = await variantStock(variantId);
      if (result.error) setError(result.error);
      if (result.stock) {
        setStock(result.stock);
        setForm(formOf(result.stock, locations));
      }
    });
  }, [variantId, locations]);

  const save = () => {
    if (!stock) return;
    const levels: StockChange["levels"] = [];
    for (const location of locations) {
      const typed = form.counts[location.id] ?? "";
      const onHand = Number(typed);
      if (form.tracked && (typed.trim() === "" || !Number.isInteger(onHand))) {
        setError(`Enter the count at ${location.name} as a whole number.`);
        return;
      }
      const expected = stock.levels.find((l) => l.location === location.id)?.on_hand ?? null;
      levels.push({ location: location.id, onHand, expected });
    }
    startTransition(async () => {
      const result = await saveVariantStock(productId, stock, {
        tracked: form.tracked,
        continueSelling: form.continueSelling,
        levels,
      });
      if (result.stock) {
        setStock(result.stock);
        setForm(formOf(result.stock, locations));
      }
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={`Stock of ${title}`}
        subtitle="Saved at once, apart from the product's other changes."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <CheckboxInput
          label="Track stock"
          description="Untracked variants are always sellable."
          value={form.tracked}
          onChange={(tracked) => setForm((before) => ({ ...before, tracked }))}
        />
        {form.tracked && (
          <>
            <CheckboxInput
              label="Keep selling when it's out of stock"
              value={form.continueSelling}
              onChange={(continueSelling) => setForm((before) => ({ ...before, continueSelling }))}
            />
            {locations.length === 0 ? (
              <Text color="secondary">The store has no locations to keep stock at.</Text>
            ) : (
              locations.map((location) => {
                const level = stock?.levels.find((l) => l.location === location.id);
                return (
                  <NumberInput
                    key={location.id}
                    label={`On hand at ${location.name}`}
                    description={
                      level && level.reserved > 0
                        ? `${level.reserved} reserved for orders, ${level.available} available.`
                        : undefined
                    }
                    value={
                      form.counts[location.id] === "" ? undefined : Number(form.counts[location.id])
                    }
                    onChange={(count) =>
                      setForm((before) => ({
                        ...before,
                        counts: {
                          ...before.counts,
                          [location.id]: count == null ? "" : String(count),
                        },
                      }))
                    }
                    min={0}
                    step={1}
                    isDisabled={isLoading}
                    width={160}
                  />
                );
              })
            )}
          </>
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save stock"
            variant="primary"
            isLoading={isPending}
            isDisabled={!stock || isLoading}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function formOf(stock: Stock | undefined, locations: Location[]) {
  return {
    tracked: stock ? stock.id != null && stock.tracked : false,
    continueSelling: stock?.continue_selling ?? false,
    counts: Object.fromEntries(
      locations.map((location) => [
        location.id,
        String(stock?.levels.find((l) => l.location === location.id)?.on_hand ?? 0),
      ]),
    ) as Record<string, string>,
  };
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
  const [price, setPrice] = useState(() => kept(current)[0]?.price ?? "");

  const taken = new Set(
    kept(draft).map((v) =>
      draft.options.map((o) => v.selections[o.key] ?? valuesOf(o)[0]?.key).join("\n"),
    ),
  );
  const isTaken = taken.has(draft.options.map((o) => selections[o.key]).join("\n"));
  const isComplete =
    draft.options.every((option) => selections[option.key]) && price.trim() !== "" && !isTaken;
  const submit = () => {
    if (!isComplete) return;
    onAdd({
      options: draft.options,
      variants: [
        ...draft.variants,
        { key: newKey(), sku, price, compareAt: "", weight: "", ships: true, selections },
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
        {isTaken && <Banner status="warning" title="A variant already has these values." />}
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
