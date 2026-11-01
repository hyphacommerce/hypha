"use client";

// A purchase order's body: its lines, extra costs, totals and details, each changed in
// place through a dialog while the order is a draft or ordered. Every change is one
// PATCH of what it touches (lines and costs are sent whole), and the page reloads with it.

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
import { DateTime } from "@/components/date-time";
import { ProductPicker } from "@/components/product-picker";
import { type PickerVariant, pickerVariants } from "@/components/product-picker-actions";
import {
  formatMoney,
  formatUnitCost,
  moneyInput,
  parseMoney,
  parseUnitCost,
  unitCostInput,
} from "@/lib/format";
import {
  ALLOCATIONS,
  COST_KINDS,
  type PurchaseOrder,
  type PurchaseOrderCost,
  type PurchaseOrderLine,
  convertMinor,
  isOpen,
  lineLabel,
  termsLabel,
} from "@/lib/purchasing";

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
  return (
    <div className="detail-columns">
      <div>
        <LinesCard po={po} canEdit={canEdit} />
        <CostsCard po={po} canEdit={canEdit} />
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
  const currencies = useCurrencies();
  const [isPicking, setIsPicking] = useState(false);
  const [picked, setPicked] = useState<{ title: string; variants: PickerVariant[] }>();
  const [editing, setEditing] = useState<LineEditing>();
  const [removing, setRemoving] = useState<PurchaseOrderLine>();
  const { error, isPending, save } = usePatch(po.id);
  const ordered = po.status !== "draft";
  const add = canEdit && (
    <Button label="Add line" variant="secondary" size="sm" onClick={() => setIsPicking(true)} />
  );
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Lines
          </Text>
          {add}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {po.lines.length === 0 ? (
          <Text type="supporting" color="secondary">
            No lines yet. Add the variants to order: prices start from the supplier's catalogue.
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Variant</TableHeaderCell>
                <TableHeaderCell>Quantity</TableHeaderCell>
                <TableHeaderCell>Unit cost</TableHeaderCell>
                {ordered && <TableHeaderCell>Arrived</TableHeaderCell>}
                {ordered && <TableHeaderCell>Outstanding</TableHeaderCell>}
                <TableHeaderCell>Total</TableHeaderCell>
                <TableHeaderCell>Landed</TableHeaderCell>
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
                            l.sku && `SKU ${l.sku}`,
                            l.supplier_sku && `Their code ${l.supplier_sku}`,
                          ]
                            .filter(Boolean)
                            .join(" · ") || "No SKU"}
                        </Text>
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <Text>{l.quantity}</Text>
                    </TableCell>
                    <TableCell>
                      <Text>{formatUnitCost(l.unit_cost, currencies)}</Text>
                    </TableCell>
                    {ordered && (
                      <TableCell>
                        <Text>
                          {l.received}
                          {l.rejected > 0 && ` (+${l.rejected} rejected)`}
                        </Text>
                      </TableCell>
                    )}
                    {ordered && (
                      <TableCell>
                        <Text>
                          {l.outstanding}
                          {l.cancelled > 0 && ` (${l.cancelled} cancelled)`}
                        </Text>
                      </TableCell>
                    )}
                    <TableCell>
                      <Text>{formatMoney(l.total, currencies)}</Text>
                    </TableCell>
                    <TableCell>
                      <Text color={l.landed_unit_cost ? undefined : "secondary"}>
                        {l.landed_unit_cost ? formatUnitCost(l.landed_unit_cost, currencies) : "—"}
                      </Text>
                    </TableCell>
                    {canEdit && (
                      <TableCell>
                        <HStack gap={1} hAlign="end">
                          <IconButton
                            label={`Edit ${lineLabel(l)}`}
                            icon={<PencilSquareIcon />}
                            size="sm"
                            variant="ghost"
                            onClick={() =>
                              setEditing({ variant: l.variant, label: lineLabel(l), line: l })
                            }
                          />
                          <IconButton
                            label={`Remove ${lineLabel(l)}`}
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
            Landed: one unit's cost with its share of the extra costs, in{" "}
            {po.store_currency ?? "the store's currency"}.
          </Text>
        )}
      </VStack>
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
        title={removing ? `Remove ${lineLabel(removing)}?` : ""}
        description="It's taken off this purchase order."
        actionLabel="Remove"
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
  const [chosen, setChosen] = useState<string>();
  const variant = product.variants.find((v) => v.id === chosen);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={product.title}
        subtitle="Which variant are you ordering?"
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label="Variant"
          options={product.variants.map((v) => ({
            value: v.id,
            label: `${v.title ?? "Default"}${v.sku ? ` · ${v.sku}` : ""}${onOrder.has(v.id) ? " (on the order)" : ""}`,
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
      supplier_sku: sku.trim() || null,
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
        subtitle={line ? "Change what's ordered." : "How many, and at what price."}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <TextInput
            label="Quantity"
            value={quantity}
            onChange={setQuantity}
            status={
              quantityOk
                ? undefined
                : {
                    type: "error",
                    message: arrived > 0 ? `At least the ${arrived} already arrived` : "At least 1",
                  }
            }
          />
          <TextInput
            label={`Unit cost (${po.currency})`}
            isOptional={!line}
            description={
              line
                ? "Net of tax, up to 4 decimals past the cent."
                : "Blank takes the supplier's price."
            }
            value={cost}
            onChange={setCost}
            status={costOk ? undefined : { type: "error", message: "An amount like 4.125" }}
          />
          <TextInput label="Their code" isOptional value={sku} onChange={setSku} />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={line ? "Save" : "Add line"}
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

function kindLabel(kind: string): string {
  return COST_KINDS.find((c) => c.value === kind)?.label ?? kind;
}

function allocationLabel(allocation: string): string {
  return ALLOCATIONS.find((a) => a.value === allocation)?.label ?? allocation;
}

function CostsCard({ po, canEdit }: { po: PurchaseOrder; canEdit: boolean }) {
  const currencies = useCurrencies();
  const [editing, setEditing] = useState<{ index: number | null }>();
  const [removing, setRemoving] = useState<number>();
  const { error, isPending, save } = usePatch(po.id);
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <VStack gap={0}>
            <Text type="display-3" as="h2">
              Extra costs
            </Text>
            <Text type="supporting" color="secondary">
              Shipping, duties and fees, spread into each unit's landed cost.
            </Text>
          </VStack>
          {canEdit && po.costs.length < 20 && (
            <Button
              label="Add cost"
              variant="secondary"
              size="sm"
              onClick={() => setEditing({ index: null })}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {po.costs.length === 0 ? (
          <Text type="supporting" color="secondary">
            None.
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Cost</TableHeaderCell>
                <TableHeaderCell>Spread</TableHeaderCell>
                <TableHeaderCell>Amount</TableHeaderCell>
                {canEdit && <TableHeaderCell />}
              </TableRow>
            </TableHeader>
            <TableBody>
              {po.costs.map((c, index) => (
                // oxlint-disable-next-line react/no-array-index-key -- costs have no ID; their position is what the API keeps
                <TableRow key={index}>
                  <TableCell>
                    <VStack gap={0}>
                      <Text weight="medium">{kindLabel(c.kind)}</Text>
                      {c.description && (
                        <Text type="supporting" color="secondary">
                          {c.description}
                        </Text>
                      )}
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Text>{allocationLabel(c.allocation)}</Text>
                  </TableCell>
                  <TableCell>
                    <Text>{formatMoney(c.amount, currencies)}</Text>
                  </TableCell>
                  {canEdit && (
                    <TableCell>
                      <HStack gap={1} hAlign="end">
                        <IconButton
                          label={`Edit ${kindLabel(c.kind)}`}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setEditing({ index })}
                        />
                        <IconButton
                          label={`Remove ${kindLabel(c.kind)}`}
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
        title="Remove this cost?"
        description="Its share comes out of the lines' landed costs."
        actionLabel="Remove"
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
        title={cost ? "Change cost" : "Add cost"}
        subtitle="Paid to bring the goods in, besides their price."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <div className="form-grid">
          <Selector label="Kind" options={COST_KINDS} value={kind} onChange={setKind} />
          <TextInput
            label="Description"
            isOptional
            value={description}
            onChange={setDescription}
            placeholder="DHL Express"
          />
          {choices.length > 1 && (
            <Selector
              label="Paid in"
              description="Duties paid at home are often in the store's currency."
              options={choices.map((c) => ({ value: c, label: c }))}
              value={currency}
              onChange={setCurrency}
            />
          )}
          <TextInput
            label={`Amount (${currency})`}
            value={amount}
            onChange={setAmount}
            status={
              amount.trim() === "" || parsed !== null
                ? undefined
                : { type: "error", message: "An amount like 25.00" }
            }
          />
          <Selector
            label="Spread over the lines"
            options={ALLOCATIONS.map((a) => ({ value: a.value, label: a.label }))}
            description={ALLOCATIONS.find((a) => a.value === allocation)?.description}
            value={allocation}
            onChange={setAllocation}
          />
        </div>
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save"
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
        <MetadataList title="Totals">
          <MetadataListItem label="Lines">
            <Text>{formatMoney(po.subtotal, currencies)}</Text>
          </MetadataListItem>
          <MetadataListItem label="Extra costs">
            <Text>{formatMoney(po.costs_total, currencies)}</Text>
          </MetadataListItem>
          <MetadataListItem label="Tax">
            <HStack gap={2} vAlign="center">
              <Text>{po.tax ? formatMoney(po.tax, currencies) : "—"}</Text>
              {canEdit && (
                <Link onClick={() => setIsEditingTax(true)} color="primary">
                  Edit
                </Link>
              )}
            </HStack>
          </MetadataListItem>
          <MetadataListItem label="Total">
            <Text weight="bold">{formatMoney(po.total, currencies)}</Text>
          </MetadataListItem>
          {inStore && (
            <MetadataListItem label={`In ${inStore.currency}`}>
              <Text>≈ {formatMoney(inStore, currencies)}</Text>
            </MetadataListItem>
          )}
          {po.costs_in_store_currency && (
            <MetadataListItem label={`Costs paid in ${po.costs_in_store_currency.currency}`}>
              <Text>{formatMoney(po.costs_in_store_currency, currencies)}</Text>
            </MetadataListItem>
          )}
        </MetadataList>
      </VStack>
      {isEditingTax && <TaxDialog po={po} onClose={() => setIsEditingTax(false)} />}
    </Card>
  );
}

function TaxDialog({ po, onClose }: { po: PurchaseOrder; onClose: () => void }) {
  const currencies = useCurrencies();
  const [amount, setAmount] = useState(() => (po.tax ? moneyInput(po.tax, currencies) : ""));
  const { error, isPending, save } = usePatch(po.id);
  const parsed = amount.trim() === "" ? null : parseMoney(amount, po.currency, currencies);
  const isValid = amount.trim() === "" || parsed !== null;
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={400} purpose="form">
      <DialogHeader
        title="Tax"
        subtitle="On the total only: purchase tax is in no unit's cost."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={`Tax (${po.currency})`}
          isOptional
          value={amount}
          onChange={setAmount}
          status={isValid ? undefined : { type: "error", message: "An amount like 25.00" }}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save"
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
  const [isEditing, setIsEditing] = useState(false);
  const place = locations.find((l) => l.id === po.destination);
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Details
          </Text>
          {canEdit && (
            <Button label="Edit" variant="ghost" size="sm" onClick={() => setIsEditing(true)} />
          )}
        </HStack>
        <MetadataList>
          <Fact label="Supplier">
            <Link href={`/suppliers/${po.supplier}`} color="primary">
              {po.supplier_name}
            </Link>
          </Fact>
          <Fact label="Goes to">{po.destination ? plain(place?.name ?? "A location") : null}</Fact>
          <Fact label="Expected">
            {po.expected_at ? <DateTime value={po.expected_at} /> : null}
          </Fact>
          <Fact label="Currency">
            <Text>{po.currency}</Text>
          </Fact>
          {po.store_currency && po.store_currency !== po.currency && (
            <Fact label="Exchange rate">
              {po.exchange_rate ? (
                <Text>
                  1 {po.currency} = {po.exchange_rate} {po.store_currency}
                </Text>
              ) : (
                <Text color="secondary">Needed to order</Text>
              )}
            </Fact>
          )}
          <Fact label="Payment terms">
            <Text>{termsLabel(po.payment_terms)}</Text>
          </Fact>
          <Fact label="Their reference">{plain(po.supplier_reference)}</Fact>
          <Fact label="Note to supplier">{plain(po.note_to_supplier)}</Fact>
          <Fact label="Internal note">{plain(po.note)}</Fact>
          {po.ordered_at && (
            <Fact label="Ordered">
              <DateTime value={po.ordered_at} />
            </Fact>
          )}
          {po.closed_at && (
            <Fact label="Closed">
              <DateTime value={po.closed_at} />
            </Fact>
          )}
          {po.cancelled_at && (
            <Fact label="Cancelled">
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
        title="Purchase order details"
        subtitle="Where it goes, when it's due, and what the supplier reads."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <Selector
            label="Goes to"
            options={locations.map((l) => ({ value: l.id, label: l.name }))}
            value={destination || undefined}
            onChange={setDestination}
            placeholder="Choose a location"
          />
          <DateInput
            label="Expected"
            isOptional
            value={expected}
            hasClear
            weekStartsOn="mon"
            onChange={(value) => setExpected(value ?? undefined)}
          />
          {needsRate && (
            <TextInput
              label={`Exchange rate (1 ${po.currency} in ${po.store_currency})`}
              description="Up to 8 decimals. Each receipt keeps the rate it used."
              value={rate}
              onChange={setRate}
              status={rateOk ? undefined : { type: "error", message: "A rate like 0.9213" }}
            />
          )}
          <TextInput
            label="Their reference"
            isOptional
            description="The supplier's own number for the order."
            value={reference}
            onChange={setReference}
          />
        </div>
        <TextArea
          label="Note to supplier"
          isOptional
          description="Printed and emailed with the order."
          value={toSupplier}
          onChange={setToSupplier}
        />
        <TextArea
          label="Internal note"
          isOptional
          description="For staff only."
          value={note}
          onChange={setNote}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save"
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
