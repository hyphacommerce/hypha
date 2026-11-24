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
import { parseUnitCost, unitCostInput } from "@/lib/format";
import { type CatalogueRow, daysLabel, variantLabel } from "@/lib/purchasing";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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
  const t = useT<"suppliers" | "purchasing">();
  const format = useFormat();
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
      header: t("common.variant"),
      width: proportional(3, { minWidth: 220 }),
      renderCell: (r) => (
        <VStack gap={0}>
          <Text weight="medium">{variantLabel(r)}</Text>
          {r.sku && (
            <Text type="supporting" color="secondary">
              {t("common.sku_named", { sku: r.sku })}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "supplier_sku",
      header: t("suppliers.their_code"),
      width: pixel(140),
      renderCell: (r) => <Text>{r.supplier_sku ?? "—"}</Text>,
    },
    {
      key: "unit_cost",
      header: t("common.price"),
      width: pixel(130),
      align: "end",
      renderCell: (r) => <Text>{r.unit_cost ? format.unitCost(r.unit_cost) : "—"}</Text>,
    },
    {
      key: "moq",
      header: t("suppliers.minimum"),
      width: pixel(100),
      align: "end",
      renderCell: (r) => (
        <Text>{r.min_order_quantity == null ? "—" : format.number(r.min_order_quantity)}</Text>
      ),
    },
    {
      key: "lead",
      header: t("suppliers.lead_time"),
      width: pixel(110),
      renderCell: (r) => <Text>{daysLabel(t, r.lead_time_days)}</Text>,
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
            label={t("common.edit_named", { name: variantLabel(r) })}
            icon={<PencilSquareIcon />}
            size="sm"
            variant="ghost"
            onClick={() => setEditing({ variant: r.variant, label: variantLabel(r), row: r })}
          />
          <IconButton
            label={t("common.remove_named", { name: variantLabel(r) })}
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
    <Button
      label={t("suppliers.add_variant")}
      variant="secondary"
      onClick={() => setIsPicking(true)}
    />
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
          title: t("suppliers.catalogue_empty_title"),
          description: t("suppliers.catalogue_empty_description"),
          action: add || undefined,
        }}
      />
      {isPicking && (
        <ProductPicker
          title={t("suppliers.picker_title")}
          mode="single"
          confirmLabel={() => t("suppliers.picker_confirm")}
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
        title={removing ? t("suppliers.remove_row_title", { name: variantLabel(removing) }) : ""}
        description={t("suppliers.remove_row_description")}
        actionLabel={t("common.remove")}
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
  const t = useT<"suppliers">();
  const [chosen, setChosen] = useState<string>();
  const variant = product.variants.find((v) => v.id === chosen);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={product.title}
        subtitle={t("suppliers.choice_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label={t("common.variant")}
          options={product.variants.map((v) => {
            const name = [v.title ?? t("common.default"), v.sku].filter(Boolean).join(" · ");
            return {
              value: v.id,
              label: listed.has(v.id) ? t("suppliers.variant_listed", { variant: name }) : name,
            };
          })}
          value={chosen}
          onChange={setChosen}
          placeholder={t("common.choose_variant")}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.next")}
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
  const t = useT<"suppliers">();
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
        subtitle={t("suppliers.row_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <TextInput label={t("suppliers.their_code")} isOptional value={sku} onChange={setSku} />
          <TextInput
            label={t("suppliers.price_per_unit", { currency })}
            isOptional
            description={t("suppliers.price_description")}
            value={cost}
            onChange={setCost}
            status={
              cost.trim() === "" || parsedCost !== null
                ? undefined
                : { type: "error", message: t("suppliers.price_invalid") }
            }
          />
          <TextInput
            label={t("suppliers.minimum_order")}
            isOptional
            value={moq}
            onChange={setMoq}
            status={
              moqValue === undefined
                ? { type: "error", message: t("suppliers.minimum_order_invalid") }
                : undefined
            }
          />
          <TextInput
            label={t("suppliers.lead_time_days")}
            isOptional
            description={t("suppliers.lead_time_overrides")}
            value={lead}
            onChange={setLead}
            status={
              leadValue === undefined
                ? { type: "error", message: t("suppliers.lead_time_invalid") }
                : undefined
            }
          />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
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
