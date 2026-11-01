"use client";

// A supplier's catalogue: each variant they sell, their code for it, their price, the
// least they sell and how long it takes. Rows are added through the product picker, then
// edited or removed in place.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { ListBulletIcon, PencilSquareIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { ProductPicker } from "@/components/product-picker";
import { type PickerVariant, pickerVariants } from "@/components/product-picker-actions";
import { ResourceTable } from "@/components/resource-table/resource-table";
import { formatUnitCost, parseUnitCost, unitCostInput } from "@/lib/format";
import { type CatalogueRow, daysLabel, variantLabel } from "@/lib/purchasing";

import { removeCatalogueRow, setCatalogueRow } from "../actions";

/** The row being edited: an existing one, or a variant just picked. */
type Editing = { variant: string; label: string; row: CatalogueRow | null };

export function CatalogueTable({
  supplier,
  currency,
  page,
  canWrite,
}: {
  supplier?: string;
  currency?: string;
  /** The page of rows; absent while it loads. */
  page?: { items: CatalogueRow[]; next_cursor: string | null; error?: string };
  canWrite: boolean;
}) {
  const currencies = useCurrencies();
  const [isPicking, setIsPicking] = useState(false);
  const [pickedProduct, setPickedProduct] = useState<{
    title: string;
    variants: PickerVariant[];
  }>();
  const [editing, setEditing] = useState<Editing>();
  const [removing, setRemoving] = useState<CatalogueRow>();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const columns: TableColumn<CatalogueRow>[] = [
    {
      key: "variant",
      header: "Variant",
      width: proportional(3, { minWidth: 220 }),
      renderCell: (r) => (
        <VStack gap={0}>
          <Text weight="medium">{variantLabel(r)}</Text>
          {r.sku && (
            <Text type="supporting" color="secondary">
              SKU {r.sku}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "supplier_sku",
      header: "Their code",
      width: pixel(140),
      renderCell: (r) => <Text>{r.supplier_sku ?? "—"}</Text>,
    },
    {
      key: "unit_cost",
      header: "Price",
      width: pixel(130),
      align: "end",
      renderCell: (r) => <Text>{r.unit_cost ? formatUnitCost(r.unit_cost, currencies) : "—"}</Text>,
    },
    {
      key: "moq",
      header: "Minimum",
      width: pixel(100),
      align: "end",
      renderCell: (r) => <Text>{r.min_order_quantity ?? "—"}</Text>,
    },
    {
      key: "lead",
      header: "Lead time",
      width: pixel(110),
      renderCell: (r) => <Text>{daysLabel(r.lead_time_days)}</Text>,
    },
  ];
  if (canWrite) {
    columns.push({
      key: "actions",
      header: "",
      width: pixel(96),
      align: "end",
      renderCell: (r) => (
        <HStack gap={1} hAlign="end">
          <IconButton
            label={`Edit ${variantLabel(r)}`}
            icon={<PencilSquareIcon />}
            size="sm"
            variant="ghost"
            onClick={() => setEditing({ variant: r.variant, label: variantLabel(r), row: r })}
          />
          <IconButton
            label={`Remove ${variantLabel(r)}`}
            icon={<TrashIcon />}
            size="sm"
            variant="ghost"
            onClick={() => setRemoving(r)}
          />
        </HStack>
      ),
    });
  }

  const add = canWrite && supplier && (
    <Button label="Add variant" variant="secondary" onClick={() => setIsPicking(true)} />
  );

  return (
    <VStack gap={3}>
      {error && <Banner status="error" title={error} />}
      <ResourceTable<CatalogueRow>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns}
        idKey="variant"
        isSelectable={false}
        error={page?.error}
        toolbarEnd={add}
        empty={{
          icon: <ListBulletIcon width={32} />,
          title: "Nothing listed yet",
          description:
            "Add the variants this supplier sells, with their code and price: purchase orders start from them.",
          action: add || undefined,
        }}
      />
      {isPicking && (
        <ProductPicker
          title="Add a product's variant"
          mode="single"
          confirmLabel={() => "Choose variant"}
          onClose={() => setIsPicking(false)}
          onConfirm={async ([product]) => {
            if (!product) return;
            const variants = await pickerVariants(product.id);
            setIsPicking(false);
            if (variants.length === 1 && variants[0]) {
              setEditing({ variant: variants[0].id, label: product.title, row: null });
            } else {
              setPickedProduct({ title: product.title, variants });
            }
          }}
        />
      )}
      {pickedProduct && (
        <VariantChoice
          product={pickedProduct}
          listed={new Set(page?.items.map((r) => r.variant))}
          onClose={() => setPickedProduct(undefined)}
          onChoose={(v) => {
            setPickedProduct(undefined);
            setEditing({
              variant: v.id,
              label: v.title ? `${pickedProduct.title} · ${v.title}` : pickedProduct.title,
              row: null,
            });
          }}
        />
      )}
      {editing && supplier && currency && (
        <RowDialog
          supplier={supplier}
          currency={currency}
          editing={editing}
          onClose={() => setEditing(undefined)}
        />
      )}
      <AlertDialog
        isOpen={removing !== undefined}
        onOpenChange={(open) => !open && setRemoving(undefined)}
        title={removing ? `Remove ${variantLabel(removing)}?` : ""}
        description="It's taken off this supplier's catalogue. Purchase orders keep what they copied."
        actionLabel="Remove"
        isActionLoading={isPending}
        onAction={() =>
          startTransition(async () => {
            if (!removing || !supplier) return;
            const result = await removeCatalogueRow(supplier, removing.variant);
            setRemoving(undefined);
            setError(result.error);
          })
        }
      />
    </VStack>
  );
}

/** Which of a product's variants the supplier sells. */
function VariantChoice({
  product,
  listed,
  onClose,
  onChoose,
}: {
  product: { title: string; variants: PickerVariant[] };
  listed: Set<string>;
  onClose: () => void;
  onChoose: (variant: PickerVariant) => void;
}) {
  const [chosen, setChosen] = useState<string>();
  const variant = product.variants.find((v) => v.id === chosen);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={product.title}
        subtitle="Which variant does this supplier sell?"
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label="Variant"
          options={product.variants.map((v) => ({
            value: v.id,
            label: `${v.title ?? "Default"}${v.sku ? ` · ${v.sku}` : ""}${listed.has(v.id) ? " (listed)" : ""}`,
          }))}
          value={chosen}
          onChange={setChosen}
          placeholder="Choose a variant"
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Next"
            variant="primary"
            isDisabled={!variant}
            onClick={() => variant && onChoose(variant)}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** A whole number from `min`, or null for blank; undefined when invalid. */
function wholeValue(input: string, min: number, max: number): number | null | undefined {
  if (input.trim() === "") return null;
  if (!/^\d+$/.test(input.trim())) return undefined;
  const n = Number(input);
  return n >= min && n <= max ? n : undefined;
}

/** Sets a catalogue row: the supplier's code, price, minimum and lead time. */
function RowDialog({
  supplier,
  currency,
  editing,
  onClose,
}: {
  supplier: string;
  currency: string;
  editing: Editing;
  onClose: () => void;
}) {
  const currencies = useCurrencies();
  const row = editing.row;
  const [sku, setSku] = useState(row?.supplier_sku ?? "");
  const [cost, setCost] = useState(() =>
    row?.unit_cost ? unitCostInput(row.unit_cost, currencies) : "",
  );
  const [moq, setMoq] = useState(() => row?.min_order_quantity?.toString() ?? "");
  const [lead, setLead] = useState(() => row?.lead_time_days?.toString() ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const parsedCost = cost.trim() === "" ? null : parseUnitCost(cost, currency, currencies);
  const moqValue = wholeValue(moq, 1, 2_147_483_647);
  const leadValue = wholeValue(lead, 0, 365);
  const isValid =
    (cost.trim() === "" || parsedCost !== null) &&
    moqValue !== undefined &&
    leadValue !== undefined;
  const save = () => {
    if (!isValid) return;
    startTransition(async () => {
      const result = await setCatalogueRow(supplier, editing.variant, {
        supplier_sku: sku.trim() || null,
        unit_cost: parsedCost === null ? null : { amount: parsedCost, currency },
        min_order_quantity: moqValue ?? null,
        lead_time_days: leadValue ?? null,
      });
      if (result.error) setError(result.error);
      else onClose();
    });
  };
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={editing.label}
        subtitle="What this supplier calls it and charges for it."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <TextInput label="Their code" isOptional value={sku} onChange={setSku} />
          <TextInput
            label={`Price per unit (${currency})`}
            isOptional
            description="Up to 4 decimals past the cent."
            value={cost}
            onChange={setCost}
            status={
              cost.trim() === "" || parsedCost !== null
                ? undefined
                : { type: "error", message: "An amount like 4.125" }
            }
          />
          <TextInput
            label="Minimum order"
            isOptional
            value={moq}
            onChange={setMoq}
            status={moqValue === undefined ? { type: "error", message: "At least 1" } : undefined}
          />
          <TextInput
            label="Lead time (days)"
            isOptional
            description="Overrides the supplier's."
            value={lead}
            onChange={setLead}
            status={
              leadValue === undefined ? { type: "error", message: "0 to 365 days" } : undefined
            }
          />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save"
            variant="primary"
            isLoading={isPending}
            isDisabled={!isValid}
            onClick={save}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
