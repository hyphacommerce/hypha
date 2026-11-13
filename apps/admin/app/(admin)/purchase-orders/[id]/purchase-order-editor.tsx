"use client";

// A purchase order's body: its lines, extra costs, totals and details, each changed in
// place through a dialog while the order is a draft or ordered (a closed one's extra
// costs too). Every change is one PATCH of what it touches (lines and costs are sent
// whole), and the page reloads with it.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { DateInput } from "@astryxdesign/core/DateInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
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
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { ISODateString } from "@astryxdesign/core/utils";
import { PencilSquareIcon, TrashIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { type ReactNode, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { CalendarDate, DateTime } from "@/components/date-time";
import { ProductPicker } from "@/components/product-picker";
import { type PickerVariant, pickerVariants } from "@/components/product-picker-actions";
import { moneyInput, parseMoney, parseUnitCost, unitCostInput } from "@/lib/format";
import {
  ALLOCATIONS,
  COST_KINDS,
  type PurchaseOrder,
  type PurchaseOrderCost,
  type PurchaseOrderLine,
  allocationLabel,
  convertMinor,
  costKindLabel,
  isOpen,
  lineLabel,
  termsLabel,
} from "@/lib/purchasing";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { type PurchaseOrderPatch, updatePurchaseOrder } from "../actions";

type Place = { id: string; name: string };
type LineRequest = AdminSchemas["PurchaseOrderLineRequest"];
type CostRequest = AdminSchemas["PurchaseOrderCostRequest"];

/** The lines as the API takes them back: unchanged, they keep their cost. */
function linesOf(po: PurchaseOrder): LineRequest[] {
  return po.lines.map((l) => ({
    variant: l.variant,
    quantity: l.quantity,
    unit_cost: l.unit_cost,
  }));
}

function costsOf(po: PurchaseOrder): CostRequest[] {
  return po.costs.map((c) => ({
    kind: c.kind,
    description: c.description,
    amount: c.amount,
    allocation: c.allocation,
  }));
}

/** Runs one PATCH, reporting its error; `done` closes the dialog on success. */
function usePatch(id: string) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const save = (patch: PurchaseOrderPatch, done?: () => void) =>
    startTransition(async () => {
      const result = await updatePurchaseOrder(id, patch);
      setError(result.error);
      if (!result.error) done?.();
    });
  return { error, setError, isPending, save };
}

export function PurchaseOrderEditor({
  po,
  locations,
  isReadOnly,
}: {
  po: PurchaseOrder;
  locations: Place[];
  isReadOnly: boolean;
}) {
  const canEdit = !isReadOnly && isOpen(po);
  // A closed order's extra costs still change: a bill that comes after the goods.
  const canEditCosts = !isReadOnly && (isOpen(po) || po.status === "closed");
  return (
    <div className="detail-columns">
      <div>
        <LinesCard po={po} canEdit={canEdit} />
        <CostsCard po={po} canEdit={canEditCosts} />
      </div>
      <div>
        <SummaryCard po={po} canEdit={canEdit} />
        <DetailsCard po={po} locations={locations} canEdit={canEdit} />
      </div>
    </div>
  );
}

// ---------- lines ----------

type LineEditing = {
  variant: string;
  label: string;
  line: PurchaseOrderLine | null;
};

function LinesCard({ po, canEdit }: { po: PurchaseOrder; canEdit: boolean }) {
  const t = useT<"purchasing">();
  const format = useFormat();
  const [isPicking, setIsPicking] = useState(false);
  const [picked, setPicked] = useState<{ title: string; variants: PickerVariant[] }>();
  const [editing, setEditing] = useState<LineEditing>();
  const [removing, setRemoving] = useState<PurchaseOrderLine>();
  const { error, isPending, save } = usePatch(po.id);
  const ordered = po.status !== "draft";
  const add = canEdit && (
    <Button
      label={t("purchasing.add_line")}
      variant="secondary"
      size="sm"
      onClick={() => setIsPicking(true)}
    />
  );
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            {t("purchasing.lines_title")}
          </Text>
          {add}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {po.lines.length === 0 ? (
          <Text type="supporting" color="secondary">
            {t("purchasing.lines_empty")}
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("common.variant")}</TableHeaderCell>
                <TableHeaderCell>{t("common.quantity")}</TableHeaderCell>
                <TableHeaderCell>{t("purchasing.unit_cost")}</TableHeaderCell>
                {ordered && <TableHeaderCell>{t("purchasing.lines_arrived")}</TableHeaderCell>}
                {ordered && <TableHeaderCell>{t("purchasing.lines_outstanding")}</TableHeaderCell>}
                <TableHeaderCell>{t("common.total")}</TableHeaderCell>
                <TableHeaderCell>{t("purchasing.lines_landed")}</TableHeaderCell>
                {canEdit && <TableHeaderCell />}
              </TableRow>
            </TableHeader>
            <TableBody>
              {po.lines.map((l) => {
                const arrived = l.received + l.rejected;
                return (
                  <TableRow key={l.variant}>
                    <TableCell>
                      <VStack gap={0}>
                        <Text weight="medium">{lineLabel(l)}</Text>
                        <Text type="supporting" color="secondary">
                          {[
                            l.sku && t("purchasing.line_sku", { sku: l.sku }),
                            l.supplier_sku &&
                              t("purchasing.line_their_code", { code: l.supplier_sku }),
                          ]
                            .filter(Boolean)
                            .join(" · ") || t("purchasing.no_sku")}
                        </Text>
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <Text>{format.number(l.quantity)}</Text>
                    </TableCell>
                    <TableCell>
                      <Text>{format.unitCost(l.unit_cost)}</Text>
                    </TableCell>
                    {ordered && (
                      <TableCell>
                        <Text>
                          {l.rejected > 0
                            ? t("purchasing.line_arrived_rejected", {
                                received: l.received,
                                rejected: l.rejected,
                              })
                            : format.number(l.received)}
                        </Text>
                      </TableCell>
                    )}
                    {ordered && (
                      <TableCell>
                        <Text>
                          {l.cancelled > 0
                            ? t("purchasing.line_outstanding_cancelled", {
                                outstanding: l.outstanding,
                                cancelled: l.cancelled,
                              })
                            : format.number(l.outstanding)}
                        </Text>
                      </TableCell>
                    )}
                    <TableCell>
                      <Text>{format.money(l.total)}</Text>
                    </TableCell>
                    <TableCell>
                      <Text color={l.landed_unit_cost ? undefined : "secondary"}>
                        {l.landed_unit_cost ? format.unitCost(l.landed_unit_cost) : "—"}
                      </Text>
                    </TableCell>
                    {canEdit && (
                      <TableCell>
                        <HStack gap={1} hAlign="end">
                          <IconButton
                            label={t("purchasing.edit_named", { name: lineLabel(l) })}
                            icon={<PencilSquareIcon />}
                            size="sm"
                            variant="ghost"
                            onClick={() =>
                              setEditing({ variant: l.variant, label: lineLabel(l), line: l })
                            }
                          />
                          <IconButton
                            label={t("purchasing.remove_named", { name: lineLabel(l) })}
                            icon={<TrashIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={arrived > 0}
                            onClick={() => setRemoving(l)}
                          />
                        </HStack>
                      </TableCell>
                    )}
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
        {po.lines.some((l) => l.landed_unit_cost) && (
          <Text type="supporting" color="secondary">
            {po.store_currency
              ? t("purchasing.landed_hint", { currency: po.store_currency })
              : t("purchasing.landed_hint_store")}
          </Text>
        )}
      </VStack>
      {isPicking && (
        <ProductPicker
          title={t("purchasing.picker_title")}
          mode="single"
          confirmLabel={() => t("purchasing.picker_confirm")}
          onClose={() => setIsPicking(false)}
          onConfirm={async ([product]) => {
            if (!product) return;
            const variants = await pickerVariants(product.id);
            setIsPicking(false);
            if (variants.length === 1 && variants[0]) {
              setEditing({ variant: variants[0].id, label: product.title, line: null });
            } else {
              setPicked({ title: product.title, variants });
            }
          }}
        />
      )}
      {picked && (
        <VariantChoice
          product={picked}
          onOrder={new Set(po.lines.map((l) => l.variant))}
          onClose={() => setPicked(undefined)}
          onChoose={(v) => {
            setPicked(undefined);
            const existing = po.lines.find((l) => l.variant === v.id) ?? null;
            setEditing({
              variant: v.id,
              label: v.title ? `${picked.title} · ${v.title}` : picked.title,
              line: existing,
            });
          }}
        />
      )}
      {editing && <LineDialog po={po} editing={editing} onClose={() => setEditing(undefined)} />}
      <AlertDialog
        isOpen={removing !== undefined}
        onOpenChange={(open) => !open && setRemoving(undefined)}
        title={removing ? t("purchasing.remove_line_title", { name: lineLabel(removing) }) : ""}
        description={t("purchasing.remove_line_description")}
        actionLabel={t("common.remove")}
        isActionLoading={isPending}
        onAction={() => {
          if (!removing) return;
          save({ lines: linesOf(po).filter((l) => l.variant !== removing.variant) }, () =>
            setRemoving(undefined),
          );
        }}
      />
    </Card>
  );
}

function VariantChoice({
  product,
  onOrder,
  onClose,
  onChoose,
}: {
  product: { title: string; variants: PickerVariant[] };
  onOrder: Set<string>;
  onClose: () => void;
  onChoose: (variant: PickerVariant) => void;
}) {
  const t = useT<"purchasing">();
  const [chosen, setChosen] = useState<string>();
  const variant = product.variants.find((v) => v.id === chosen);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={product.title}
        subtitle={t("purchasing.choice_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label={t("common.variant")}
          options={product.variants.map((v) => {
            const name = [v.title ?? t("common.default"), v.sku].filter(Boolean).join(" · ");
            return {
              value: v.id,
              label: onOrder.has(v.id) ? t("purchasing.variant_on_order", { variant: name }) : name,
            };
          })}
          value={chosen}
          onChange={setChosen}
          placeholder={t("purchasing.choose_variant")}
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

/** Adds a line or changes one: its quantity, unit cost and the supplier's code. */
function LineDialog({
  po,
  editing,
  onClose,
}: {
  po: PurchaseOrder;
  editing: LineEditing;
  onClose: () => void;
}) {
  const t = useT<"purchasing">();
  const currencies = useCurrencies();
  const line = editing.line;
  const [quantity, setQuantity] = useState(() => line?.quantity.toString() ?? "1");
  const [cost, setCost] = useState(() => (line ? unitCostInput(line.unit_cost, currencies) : ""));
  const [sku, setSku] = useState(line?.supplier_sku ?? "");
  const { error, isPending, save } = usePatch(po.id);
  const arrived = line ? line.received + line.rejected : 0;
  const count = /^\d+$/.test(quantity.trim()) ? Number(quantity) : NaN;
  const quantityOk = Number.isSafeInteger(count) && count >= Math.max(1, arrived);
  const parsedCost = cost.trim() === "" ? null : parseUnitCost(cost, po.currency, currencies);
  const costOk = cost.trim() === "" ? !line : parsedCost !== null;
  const submit = () => {
    if (!quantityOk || !costOk) return;
    const request: LineRequest = {
      variant: editing.variant,
      quantity: count,
      ...(parsedCost !== null ? { unit_cost: { amount: parsedCost, currency: po.currency } } : {}),
      // Blank on a new line leaves it to the server, which takes the supplier's code from
      // its catalogue or last order; on a line that has one, blank clears it.
      ...(sku.trim() ? { supplier_sku: sku.trim() } : line ? { supplier_sku: null } : {}),
    };
    const lines = linesOf(po).filter((l) => l.variant !== editing.variant);
    const at = po.lines.findIndex((l) => l.variant === editing.variant);
    if (at >= 0) lines.splice(at, 0, request);
    else lines.push(request);
    save({ lines }, onClose);
  };
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={editing.label}
        subtitle={line ? t("purchasing.line_subtitle_change") : t("purchasing.line_subtitle_new")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <TextInput
            label={t("common.quantity")}
            value={quantity}
            onChange={setQuantity}
            status={
              quantityOk
                ? undefined
                : {
                    type: "error",
                    message:
                      arrived > 0
                        ? t("purchasing.quantity_min_arrived", { count: arrived })
                        : t("purchasing.quantity_min"),
                  }
            }
          />
          <TextInput
            label={t("purchasing.unit_cost_in", { currency: po.currency })}
            isOptional={!line}
            description={
              line
                ? t("purchasing.unit_cost_description_change")
                : t("purchasing.unit_cost_description_new")
            }
            value={cost}
            onChange={setCost}
            status={
              costOk ? undefined : { type: "error", message: t("purchasing.unit_cost_invalid") }
            }
          />
          <TextInput label={t("purchasing.their_code")} isOptional value={sku} onChange={setSku} />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={line ? t("common.save") : t("purchasing.line_add_submit")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!quantityOk || !costOk}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

// ---------- extra costs ----------

function CostsCard({ po, canEdit }: { po: PurchaseOrder; canEdit: boolean }) {
  const t = useT<"purchasing">();
  const format = useFormat();
  const [editing, setEditing] = useState<{ index: number | null }>();
  const [removing, setRemoving] = useState<number>();
  const { error, isPending, save } = usePatch(po.id);
  // Units already in stock: a change to the costs revalues them.
  const arrived = po.lines.reduce((sum, l) => sum + l.received, 0);
  const revalues = canEdit && arrived > 0;
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <VStack gap={0}>
            <Text type="display-3" as="h2">
              {t("purchasing.costs_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("purchasing.costs_subtitle")}
            </Text>
          </VStack>
          {canEdit && po.costs.length < 20 && (
            <Button
              label={t("purchasing.add_cost")}
              variant="secondary"
              size="sm"
              onClick={() => setEditing({ index: null })}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {revalues && (
          <Banner
            status="info"
            title={t("purchasing.costs_arrived_title", { count: arrived })}
            description={t("purchasing.costs_arrived_description")}
          />
        )}
        {po.costs.length === 0 ? (
          <Text type="supporting" color="secondary">
            {t("purchasing.costs_none")}
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("purchasing.costs_cost")}</TableHeaderCell>
                <TableHeaderCell>{t("purchasing.costs_spread")}</TableHeaderCell>
                <TableHeaderCell>{t("common.amount")}</TableHeaderCell>
                {canEdit && <TableHeaderCell />}
              </TableRow>
            </TableHeader>
            <TableBody>
              {po.costs.map((c, index) => (
                // oxlint-disable-next-line react/no-array-index-key -- costs have no ID; their position is what the API keeps
                <TableRow key={index}>
                  <TableCell>
                    <VStack gap={0}>
                      <Text weight="medium">{costKindLabel(t, c.kind)}</Text>
                      {c.description && (
                        <Text type="supporting" color="secondary">
                          {c.description}
                        </Text>
                      )}
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Text>{allocationLabel(t, c.allocation)}</Text>
                  </TableCell>
                  <TableCell>
                    <Text>{format.money(c.amount)}</Text>
                  </TableCell>
                  {canEdit && (
                    <TableCell>
                      <HStack gap={1} hAlign="end">
                        <IconButton
                          label={t("purchasing.edit_named", { name: costKindLabel(t, c.kind) })}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setEditing({ index })}
                        />
                        <IconButton
                          label={t("purchasing.remove_named", { name: costKindLabel(t, c.kind) })}
                          icon={<TrashIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setRemoving(index)}
                        />
                      </HStack>
                    </TableCell>
                  )}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {editing && (
        <CostDialog
          po={po}
          cost={editing.index === null ? null : (po.costs[editing.index] ?? null)}
          onSave={(cost, done) => {
            const costs = costsOf(po);
            if (editing.index === null) costs.push(cost);
            else costs[editing.index] = cost;
            save({ costs }, done);
          }}
          onClose={() => setEditing(undefined)}
        />
      )}
      <AlertDialog
        isOpen={removing !== undefined}
        onOpenChange={(open) => !open && setRemoving(undefined)}
        title={t("purchasing.remove_cost_title")}
        description={
          revalues
            ? t("purchasing.remove_cost_description_stock")
            : t("purchasing.remove_cost_description")
        }
        actionLabel={t("common.remove")}
        isActionLoading={isPending}
        onAction={() =>
          save({ costs: costsOf(po).filter((_, i) => i !== removing) }, () =>
            setRemoving(undefined),
          )
        }
      />
    </Card>
  );
}

function CostDialog({
  po,
  cost,
  onSave,
  onClose,
}: {
  po: PurchaseOrder;
  cost: PurchaseOrderCost | null;
  onSave: (cost: CostRequest, done: () => void) => void;
  onClose: () => void;
}) {
  const t = useT<"purchasing">();
  const currencies = useCurrencies();
  const [kind, setKind] = useState<CostRequest["kind"]>(cost?.kind ?? "shipping");
  const [description, setDescription] = useState(cost?.description ?? "");
  const [currency, setCurrency] = useState(cost?.amount.currency ?? po.currency);
  const [amount, setAmount] = useState(() => (cost ? moneyInput(cost.amount, currencies) : ""));
  const [allocation, setAllocation] = useState<CostRequest["allocation"]>(
    cost?.allocation ?? "by_value",
  );
  const [isPending, startTransition] = useTransition();
  const parsed = parseMoney(amount, currency, currencies);
  const spread = ALLOCATIONS.find((a) => a === allocation);
  const choices = [
    po.currency,
    ...(po.store_currency && po.store_currency !== po.currency ? [po.store_currency] : []),
  ];
  const submit = () => {
    if (parsed === null) return;
    startTransition(() =>
      onSave(
        {
          kind,
          description: description.trim() || null,
          amount: { amount: parsed, currency },
          allocation,
        },
        onClose,
      ),
    );
  };
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={cost ? t("purchasing.change_cost") : t("purchasing.cost_dialog_title_add")}
        subtitle={t("purchasing.cost_dialog_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <div className="form-grid">
          <Selector
            label={t("purchasing.cost_kind")}
            options={COST_KINDS.map((value) => ({ value, label: costKindLabel(t, value) }))}
            value={kind}
            onChange={setKind}
          />
          <TextInput
            label={t("common.description")}
            isOptional
            value={description}
            onChange={setDescription}
            placeholder={t("purchasing.cost_description_placeholder")}
          />
          {choices.length > 1 && (
            <Selector
              label={t("purchasing.cost_paid_in")}
              description={t("purchasing.cost_paid_in_description")}
              options={choices.map((c) => ({ value: c, label: c }))}
              value={currency}
              onChange={setCurrency}
            />
          )}
          <TextInput
            label={t("purchasing.amount_in", { currency })}
            value={amount}
            onChange={setAmount}
            status={
              amount.trim() === "" || parsed !== null
                ? undefined
                : { type: "error", message: t("purchasing.amount_invalid") }
            }
          />
          <Selector
            label={t("purchasing.cost_spread_label")}
            options={ALLOCATIONS.map((value) => ({ value, label: allocationLabel(t, value) }))}
            description={spread && t(`purchasing.allocation_${spread}_description`)}
            value={allocation}
            onChange={setAllocation}
          />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
            variant="primary"
            isLoading={isPending}
            isDisabled={parsed === null}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

// ---------- totals ----------

function SummaryCard({ po, canEdit }: { po: PurchaseOrder; canEdit: boolean }) {
  const t = useT<"purchasing">();
  const format = useFormat();
  const currencies = useCurrencies();
  const [isEditingTax, setIsEditingTax] = useState(false);
  const store = po.store_currency;
  const inStore =
    store && po.exchange_rate && store !== po.currency && currencies[store] !== undefined
      ? {
          amount: convertMinor(
            po.total.amount,
            po.exchange_rate,
            currencies[po.currency] ?? 2,
            currencies[store] ?? 2,
          ),
          currency: store,
        }
      : null;
  return (
    <Card>
      <VStack gap={3}>
        <MetadataList title={t("purchasing.totals_title")}>
          <MetadataListItem label={t("purchasing.totals_lines")}>
            <Text>{format.money(po.subtotal)}</Text>
          </MetadataListItem>
          <MetadataListItem label={t("purchasing.totals_costs")}>
            <Text>{format.money(po.costs_total)}</Text>
          </MetadataListItem>
          <MetadataListItem label={t("common.tax")}>
            <HStack gap={2} vAlign="center">
              <Text>{po.tax ? format.money(po.tax) : "—"}</Text>
              {canEdit && (
                <Link onClick={() => setIsEditingTax(true)} color="primary">
                  {t("common.edit")}
                </Link>
              )}
            </HStack>
          </MetadataListItem>
          <MetadataListItem label={t("common.total")}>
            <Text weight="bold">{format.money(po.total)}</Text>
          </MetadataListItem>
          {inStore && (
            <MetadataListItem
              label={t("purchasing.totals_in_currency", { currency: inStore.currency })}
            >
              <Text>≈ {format.money(inStore)}</Text>
            </MetadataListItem>
          )}
          {po.costs_in_store_currency && (
            <MetadataListItem
              label={t("purchasing.totals_costs_paid_in", {
                currency: po.costs_in_store_currency.currency,
              })}
            >
              <Text>{format.money(po.costs_in_store_currency)}</Text>
            </MetadataListItem>
          )}
        </MetadataList>
      </VStack>
      {isEditingTax && <TaxDialog po={po} onClose={() => setIsEditingTax(false)} />}
    </Card>
  );
}

function TaxDialog({ po, onClose }: { po: PurchaseOrder; onClose: () => void }) {
  const t = useT<"purchasing">();
  const currencies = useCurrencies();
  const [amount, setAmount] = useState(() => (po.tax ? moneyInput(po.tax, currencies) : ""));
  const { error, isPending, save } = usePatch(po.id);
  const parsed = amount.trim() === "" ? null : parseMoney(amount, po.currency, currencies);
  const isValid = amount.trim() === "" || parsed !== null;
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={400} purpose="form">
      <DialogHeader
        title={t("common.tax")}
        subtitle={t("purchasing.tax_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("purchasing.tax_in", { currency: po.currency })}
          isOptional
          value={amount}
          onChange={setAmount}
          status={isValid ? undefined : { type: "error", message: t("purchasing.amount_invalid") }}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!isValid}
            onClick={() =>
              save(
                { tax: parsed === null ? null : { amount: parsed, currency: po.currency } },
                onClose,
              )
            }
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

// ---------- details ----------

const isDay = (value: string): value is ISODateString => /^\d{4}-\d{2}-\d{2}$/.test(value);

/** "2026-11-14" from an RFC 3339 instant, for the date input. */
function dayOf(at: string | null | undefined): ISODateString | undefined {
  const day = at?.slice(0, 10);
  return day && isDay(day) ? day : undefined;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <MetadataListItem label={label}>
      {children ?? <Text color="secondary">—</Text>}
    </MetadataListItem>
  );
}

function plain(value: string | null | undefined) {
  return value ? <Text>{value}</Text> : null;
}

function DetailsCard({
  po,
  locations,
  canEdit,
}: {
  po: PurchaseOrder;
  locations: Place[];
  canEdit: boolean;
}) {
  const t = useT<"purchasing">();
  const [isEditing, setIsEditing] = useState(false);
  const place = locations.find((l) => l.id === po.destination);
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            {t("common.details")}
          </Text>
          {canEdit && (
            <Button
              label={t("common.edit")}
              variant="ghost"
              size="sm"
              onClick={() => setIsEditing(true)}
            />
          )}
        </HStack>
        <MetadataList>
          <Fact label={t("common.supplier")}>
            <Link href={`/suppliers/${po.supplier}`} color="primary">
              {po.supplier_name}
            </Link>
          </Fact>
          <Fact label={t("purchasing.goes_to")}>
            {po.destination ? plain(place?.name ?? t("purchasing.a_location")) : null}
          </Fact>
          <Fact label={t("purchasing.expected")}>
            {po.expected_at ? <CalendarDate value={po.expected_at} /> : null}
          </Fact>
          <Fact label={t("common.currency")}>
            <Text>{po.currency}</Text>
          </Fact>
          {po.store_currency && po.store_currency !== po.currency && (
            <Fact label={t("purchasing.exchange_rate")}>
              {po.exchange_rate ? (
                <Text>
                  1 {po.currency} = {po.exchange_rate} {po.store_currency}
                </Text>
              ) : (
                <Text color="secondary">{t("purchasing.exchange_rate_needed")}</Text>
              )}
            </Fact>
          )}
          <Fact label={t("purchasing.payment_terms")}>
            <Text>{termsLabel(t, po.payment_terms)}</Text>
          </Fact>
          <Fact label={t("purchasing.their_reference")}>{plain(po.supplier_reference)}</Fact>
          <Fact label={t("purchasing.note_to_supplier")}>{plain(po.note_to_supplier)}</Fact>
          <Fact label={t("purchasing.internal_note")}>{plain(po.note)}</Fact>
          {po.ordered_at && (
            <Fact label={t("purchasing.fact_ordered")}>
              <DateTime value={po.ordered_at} />
            </Fact>
          )}
          {po.closed_at && (
            <Fact label={t("purchasing.fact_closed")}>
              <DateTime value={po.closed_at} />
            </Fact>
          )}
          {po.cancelled_at && (
            <Fact label={t("purchasing.fact_cancelled")}>
              <DateTime value={po.cancelled_at} />
            </Fact>
          )}
        </MetadataList>
      </VStack>
      {isEditing && (
        <DetailsDialog po={po} locations={locations} onClose={() => setIsEditing(false)} />
      )}
    </Card>
  );
}

function DetailsDialog({
  po,
  locations,
  onClose,
}: {
  po: PurchaseOrder;
  locations: Place[];
  onClose: () => void;
}) {
  const t = useT<"purchasing">();
  const [destination, setDestination] = useState(po.destination ?? "");
  const [expected, setExpected] = useState(() => dayOf(po.expected_at));
  const [rate, setRate] = useState(po.exchange_rate ?? "");
  const [reference, setReference] = useState(po.supplier_reference ?? "");
  const [toSupplier, setToSupplier] = useState(po.note_to_supplier ?? "");
  const [note, setNote] = useState(po.note ?? "");
  const { error, isPending, save } = usePatch(po.id);
  const needsRate = !!po.store_currency && po.store_currency !== po.currency;
  const rateOk = rate.trim() === "" || /^\d+(\.\d{1,8})?$/.test(rate.trim());
  const submit = () => {
    if (!rateOk) return;
    save(
      {
        destination: destination || null,
        // Noon UTC: the same day in every store's time zone.
        expected_at: expected ? `${expected}T12:00:00Z` : null,
        ...(needsRate ? { exchange_rate: rate.trim() || null } : {}),
        supplier_reference: reference.trim() || null,
        note_to_supplier: toSupplier.trim() || null,
        note: note.trim() || null,
      },
      onClose,
    );
  };
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={520} purpose="form">
      <DialogHeader
        title={t("purchasing.details_dialog_title")}
        subtitle={t("purchasing.details_dialog_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <Selector
            label={t("purchasing.goes_to")}
            options={locations.map((l) => ({ value: l.id, label: l.name }))}
            value={destination || undefined}
            onChange={setDestination}
            placeholder={t("purchasing.choose_location")}
          />
          <DateInput
            label={t("purchasing.expected")}
            isOptional
            value={expected}
            hasClear
            weekStartsOn="mon"
            onChange={(value) => setExpected(value ?? undefined)}
          />
          {needsRate && (
            <TextInput
              label={t("purchasing.exchange_rate_in", {
                currency: po.currency,
                store_currency: po.store_currency ?? "",
              })}
              description={t("purchasing.exchange_rate_description")}
              value={rate}
              onChange={setRate}
              status={
                rateOk
                  ? undefined
                  : { type: "error", message: t("purchasing.exchange_rate_invalid") }
              }
            />
          )}
          <TextInput
            label={t("purchasing.their_reference")}
            isOptional
            description={t("purchasing.their_reference_description")}
            value={reference}
            onChange={setReference}
          />
        </div>
        <TextArea
          label={t("purchasing.note_to_supplier")}
          isOptional
          description={t("purchasing.note_to_supplier_description")}
          value={toSupplier}
          onChange={setToSupplier}
        />
        <TextArea
          label={t("purchasing.internal_note")}
          isOptional
          description={t("purchasing.internal_note_description")}
          value={note}
          onChange={setNote}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!rateOk}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
