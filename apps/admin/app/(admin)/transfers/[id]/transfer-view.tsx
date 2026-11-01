"use client";

// A transfer's body and actions: its lines (what left, what arrived, what's on its way)
// and details, changed in place while it's a draft; shipping it whole, receiving it in
// parts, cancelling or deleting a draft. Every change is one request, and the page
// reloads with it.

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
import { NumberInput } from "@astryxdesign/core/NumberInput";
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
import { PencilSquareIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { CalendarDate, DateTime } from "@/components/date-time";
import { ProductPicker } from "@/components/product-picker";
import { type PickerVariant, pickerVariants } from "@/components/product-picker-actions";
import {
  type Transfer,
  type TransferLine,
  dayOf,
  transferLineLabel,
  trNumber,
} from "@/lib/transfers";

import {
  type TransferPatch,
  cancelTransfer,
  deleteTransfer,
  receiveTransfer,
  shipTransfer,
  updateTransfer,
} from "../actions";

type Place = { id: string; name: string };

/** Lines as a patch sends them, whole. */
function linesOf(transfer: Transfer): { variant: string; quantity: number }[] {
  return transfer.lines.map((l) => ({ variant: l.variant, quantity: l.quantity }));
}

/** A patch of the draft: its error and whether it's on its way. */
function usePatch(id: string) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const save = (patch: TransferPatch, done?: () => void) =>
    startTransition(async () => {
      const result = await updateTransfer(id, patch);
      setError(result.error);
      if (!result.error) done?.();
    });
  return { error, isPending, save };
}

export function TransferView({
  transfer,
  places,
  isReadOnly,
}: {
  transfer: Transfer;
  places: Place[];
  isReadOnly: boolean;
}) {
  const canEdit = !isReadOnly && transfer.status === "draft";
  return (
    <VStack gap={5}>
      <Lines transfer={transfer} canEdit={canEdit} />
      <Details transfer={transfer} places={places} canEdit={canEdit} />
    </VStack>
  );
}

function Lines({ transfer, canEdit }: { transfer: Transfer; canEdit: boolean }) {
  const [isPicking, setIsPicking] = useState(false);
  const [picked, setPicked] = useState<{ title: string; variants: PickerVariant[] }>();
  const [editing, setEditing] = useState<{ variant: string; label: string; quantity: number }>();
  const [removing, setRemoving] = useState<TransferLine>();
  const { error, isPending, save } = usePatch(transfer.id);
  const shipped = transfer.status !== "draft" && transfer.status !== "cancelled";
  const startEditing = (variant: string, label: string) => {
    const existing = transfer.lines.find((l) => l.variant === variant);
    setEditing({ variant, label, quantity: existing?.quantity ?? 1 });
  };
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Lines
          </Text>
          {canEdit && (
            <Button
              label="Add line"
              variant="secondary"
              size="sm"
              onClick={() => setIsPicking(true)}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {transfer.lines.length === 0 ? (
          <Text type="supporting" color="secondary">
            No lines yet. Add the variants to move: each needs stock tracked at the origin.
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Variant</TableHeaderCell>
                <TableHeaderCell>Quantity</TableHeaderCell>
                {shipped && <TableHeaderCell>Received</TableHeaderCell>}
                {shipped && <TableHeaderCell>Lost</TableHeaderCell>}
                {shipped && <TableHeaderCell>In transit</TableHeaderCell>}
                {canEdit && <TableHeaderCell />}
              </TableRow>
            </TableHeader>
            <TableBody>
              {transfer.lines.map((l) => (
                <TableRow key={l.variant}>
                  <TableCell>
                    <VStack gap={0}>
                      <Text weight="medium">{transferLineLabel(l)}</Text>
                      <Text type="supporting" color="secondary">
                        {l.sku ? `SKU ${l.sku}` : "No SKU"}
                      </Text>
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Text>{l.quantity}</Text>
                  </TableCell>
                  {shipped && (
                    <TableCell>
                      <Text>{l.received}</Text>
                    </TableCell>
                  )}
                  {shipped && (
                    <TableCell>
                      <Text color={l.rejected > 0 ? undefined : "secondary"}>{l.rejected}</Text>
                    </TableCell>
                  )}
                  {shipped && (
                    <TableCell>
                      <Text>{l.in_transit}</Text>
                    </TableCell>
                  )}
                  {canEdit && (
                    <TableCell>
                      <HStack gap={1} hAlign="end">
                        <IconButton
                          label={`Edit ${transferLineLabel(l)}`}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => startEditing(l.variant, transferLineLabel(l))}
                        />
                        <IconButton
                          label={`Remove ${transferLineLabel(l)}`}
                          icon={<TrashIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setRemoving(l)}
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
              startEditing(variants[0].id, product.title);
            } else {
              setPicked({ title: product.title, variants });
            }
          }}
        />
      )}
      {picked && (
        <VariantChoice
          product={picked}
          onTransfer={new Set(transfer.lines.map((l) => l.variant))}
          onClose={() => setPicked(undefined)}
          onChoose={(v) => {
            setPicked(undefined);
            startEditing(v.id, v.title ? `${picked.title} · ${v.title}` : picked.title);
          }}
        />
      )}
      {editing && (
        <QuantityDialog
          editing={editing}
          isPending={isPending}
          error={error}
          onClose={() => setEditing(undefined)}
          onSave={(quantity) => {
            const others = linesOf(transfer).filter((l) => l.variant !== editing.variant);
            const at = transfer.lines.findIndex((l) => l.variant === editing.variant);
            const line = { variant: editing.variant, quantity };
            const lines =
              at === -1 ? [...others, line] : [...others.slice(0, at), line, ...others.slice(at)];
            save({ lines }, () => setEditing(undefined));
          }}
        />
      )}
      <AlertDialog
        isOpen={removing !== undefined}
        onOpenChange={(open) => !open && setRemoving(undefined)}
        title={removing ? `Remove ${transferLineLabel(removing)}?` : ""}
        description="It's taken off this transfer."
        actionLabel="Remove"
        isActionLoading={isPending}
        onAction={() => {
          if (!removing) return;
          save({ lines: linesOf(transfer).filter((l) => l.variant !== removing.variant) }, () =>
            setRemoving(undefined),
          );
        }}
      />
    </Card>
  );
}

function VariantChoice({
  product,
  onTransfer,
  onClose,
  onChoose,
}: {
  product: { title: string; variants: PickerVariant[] };
  onTransfer: Set<string>;
  onClose: () => void;
  onChoose: (variant: PickerVariant) => void;
}) {
  const [chosen, setChosen] = useState<string>();
  const variant = product.variants.find((v) => v.id === chosen);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={product.title}
        subtitle="Which variant is moving?"
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label="Variant"
          options={product.variants.map((v) => ({
            value: v.id,
            label: `${v.title ?? "Default"}${v.sku ? ` · ${v.sku}` : ""}${onTransfer.has(v.id) ? " (on the transfer)" : ""}`,
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

function QuantityDialog({
  editing,
  isPending,
  error,
  onClose,
  onSave,
}: {
  editing: { label: string; quantity: number };
  isPending: boolean;
  error?: string;
  onClose: () => void;
  onSave: (quantity: number) => void;
}) {
  const [quantity, setQuantity] = useState(editing.quantity);
  const isValid = Number.isInteger(quantity) && quantity >= 1;
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={400} purpose="form">
      <DialogHeader
        title={editing.label}
        subtitle="How many units move."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <NumberInput label="Quantity" min={1} value={quantity} onChange={setQuantity} />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save"
            variant="primary"
            isLoading={isPending}
            isDisabled={!isValid}
            onClick={() => onSave(quantity)}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function Details({
  transfer,
  places,
  canEdit,
}: {
  transfer: Transfer;
  places: Place[];
  canEdit: boolean;
}) {
  const [isEditing, setIsEditing] = useState(false);
  const nameOf = (id: string) => places.find((p) => p.id === id)?.name ?? id;
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Details
          </Text>
          {canEdit && <Link onClick={() => setIsEditing(true)}>Edit</Link>}
        </HStack>
        <MetadataList>
          <MetadataListItem label="From">
            <Text>{nameOf(transfer.origin)}</Text>
          </MetadataListItem>
          <MetadataListItem label="To">
            <Text>{nameOf(transfer.destination)}</Text>
          </MetadataListItem>
          <MetadataListItem label="Expected">
            {transfer.expected_at ? (
              <CalendarDate value={transfer.expected_at} />
            ) : (
              <Text color="secondary">—</Text>
            )}
          </MetadataListItem>
          <MetadataListItem label="Reference">
            <Text color={transfer.reference ? undefined : "secondary"}>
              {transfer.reference ?? "—"}
            </Text>
          </MetadataListItem>
          {transfer.shipped_at && (
            <MetadataListItem label="Shipped">
              <DateTime value={transfer.shipped_at} />
            </MetadataListItem>
          )}
          {transfer.received_at && (
            <MetadataListItem label="Received">
              <DateTime value={transfer.received_at} />
            </MetadataListItem>
          )}
          {transfer.cancelled_at && (
            <MetadataListItem label="Cancelled">
              <DateTime value={transfer.cancelled_at} />
            </MetadataListItem>
          )}
        </MetadataList>
        {transfer.note && <Text type="supporting">{transfer.note}</Text>}
      </VStack>
      {isEditing && (
        <DetailsDialog transfer={transfer} places={places} onClose={() => setIsEditing(false)} />
      )}
    </Card>
  );
}

function DetailsDialog({
  transfer,
  places,
  onClose,
}: {
  transfer: Transfer;
  places: Place[];
  onClose: () => void;
}) {
  const [origin, setOrigin] = useState(transfer.origin);
  const [destination, setDestination] = useState(transfer.destination);
  const [expected, setExpected] = useState(() => dayOf(transfer.expected_at));
  const [reference, setReference] = useState(transfer.reference ?? "");
  const [note, setNote] = useState(transfer.note ?? "");
  const { error, isPending, save } = usePatch(transfer.id);
  const options = places.map((p) => ({ value: p.id, label: p.name }));
  const same = origin === destination;
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={520} purpose="form">
      <DialogHeader
        title="Transfer details"
        subtitle="Where it goes from and to, and when it's due."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <Selector label="From" options={options} value={origin} onChange={setOrigin} />
          <Selector
            label="To"
            options={options}
            value={destination}
            onChange={setDestination}
            status={
              same
                ? { type: "error", message: "Choose another location than where it's from" }
                : undefined
            }
          />
          <DateInput
            label="Expected"
            isOptional
            value={expected}
            hasClear
            weekStartsOn="mon"
            onChange={(value) => setExpected(value ?? undefined)}
          />
          <TextInput
            label="Reference"
            isOptional
            description="A carrier or tracking note."
            value={reference}
            onChange={setReference}
          />
        </div>
        <TextArea label="Note" isOptional value={note} onChange={setNote} />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Save"
            variant="primary"
            isLoading={isPending}
            isDisabled={same}
            onClick={() =>
              save(
                {
                  origin,
                  destination,
                  // Noon UTC: the same day in every store's time zone.
                  expected_at: expected ? `${expected}T12:00:00Z` : null,
                  reference: reference.trim() || null,
                  note: note.trim() || null,
                },
                onClose,
              )
            }
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

type Confirm = "cancel" | "delete";

/** The header's actions: a draft ships, is cancelled or deleted; one in transit is received. */
export function TransferActions({
  transfer,
  atOrigin,
  places,
}: {
  transfer: Transfer;
  /** Each line's variant's count at the origin, for shipping's warning. */
  atOrigin: Record<string, number>;
  places: Place[];
}) {
  const [confirm, setConfirm] = useState<Confirm>();
  const [isShipping, setIsShipping] = useState(false);
  const [isReceiving, setIsReceiving] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const run = (work: () => Promise<{ error?: string }>) =>
    startTransition(async () => {
      const result = await work();
      setConfirm(undefined);
      setError(result?.error);
    });
  const origin = places.find((p) => p.id === transfer.origin)?.name ?? "the origin";
  if (transfer.status !== "draft" && transfer.status !== "in_transit") return null;
  return (
    <VStack gap={2}>
      {error && <Banner status="error" title={error} />}
      <HStack gap={2} wrap="wrap">
        {transfer.status === "draft" ? (
          <>
            <Button label="Delete" variant="ghost" onClick={() => setConfirm("delete")} />
            <Button label="Cancel" variant="secondary" onClick={() => setConfirm("cancel")} />
            <Button
              label="Ship"
              variant="primary"
              isDisabled={transfer.lines.length === 0}
              onClick={() => setIsShipping(true)}
            />
          </>
        ) : (
          <Button label="Receive" variant="primary" onClick={() => setIsReceiving(true)} />
        )}
      </HStack>
      <AlertDialog
        isOpen={confirm !== undefined}
        onOpenChange={(open) => !open && setConfirm(undefined)}
        title={confirm === "delete" ? "Delete this draft?" : "Cancel this transfer?"}
        description={`${trNumber(transfer.number)}: ${
          confirm === "delete"
            ? "it's removed for good."
            : "it stays listed as cancelled; nothing moved."
        }`}
        actionLabel={confirm === "delete" ? "Delete" : "Cancel transfer"}
        isActionLoading={isPending}
        onAction={() =>
          run(() =>
            confirm === "delete" ? deleteTransfer(transfer.id) : cancelTransfer(transfer.id),
          )
        }
      />
      {isShipping && (
        <ShipDialog
          transfer={transfer}
          atOrigin={atOrigin}
          origin={origin}
          onClose={() => setIsShipping(false)}
        />
      )}
      {isReceiving && <ReceiveDialog transfer={transfer} onClose={() => setIsReceiving(false)} />}
    </VStack>
  );
}

function ShipDialog({
  transfer,
  atOrigin,
  origin,
  onClose,
}: {
  transfer: Transfer;
  atOrigin: Record<string, number>;
  origin: string;
  onClose: () => void;
}) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // One key per dialog: a double submit ships once.
  const [key] = useState(() => crypto.randomUUID());
  const short = transfer.lines.filter((l) => (atOrigin[l.variant] ?? 0) < l.quantity);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={560} purpose="form">
      <DialogHeader
        title={`Ship ${trNumber(transfer.number)}`}
        subtitle={`Every line leaves ${origin} now, whole, and is on its way until received.`}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <VStack gap={1}>
          {transfer.lines.map((l) => (
            <Text key={l.variant}>
              {l.quantity} × {transferLineLabel(l)}
              <Text type="supporting" color="secondary">
                {" "}
                · {atOrigin[l.variant] ?? 0} at {origin}
              </Text>
            </Text>
          ))}
        </VStack>
        {short.length > 0 && (
          <Banner
            status="warning"
            title="Not enough counted at the origin"
            description={`${short.map(transferLineLabel).join(", ")}: shipping takes the count below zero. Ship anyway?`}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={short.length > 0 ? "Ship anyway" : "Ship"}
            variant="primary"
            isLoading={isPending}
            onClick={() =>
              startTransition(async () => {
                const result = await shipTransfer(transfer.id, key);
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

type Counts = Record<string, { accepted: number; rejected: number }>;

function ReceiveDialog({ transfer, onClose }: { transfer: Transfer; onClose: () => void }) {
  const waiting = transfer.lines.filter((l) => l.in_transit > 0);
  const [counts, setCounts] = useState<Counts>(() =>
    Object.fromEntries(waiting.map((l) => [l.variant, { accepted: l.in_transit, rejected: 0 }])),
  );
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // One key per dialog: a double submit receives once.
  const [key] = useState(() => crypto.randomUUID());
  const set = (variant: string, change: Partial<{ accepted: number; rejected: number }>) =>
    setCounts({
      ...counts,
      [variant]: { ...(counts[variant] ?? { accepted: 0, rejected: 0 }), ...change },
    });
  const lines = waiting
    .map((l) => ({ variant: l.variant, ...(counts[l.variant] ?? { accepted: 0, rejected: 0 }) }))
    .filter((l) => l.accepted + l.rejected > 0);
  const over = waiting.filter((l) => {
    const c = counts[l.variant];
    return c !== undefined && c.accepted + c.rejected > l.in_transit;
  });
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={640} purpose="form">
      <DialogHeader
        title={`Receive ${trNumber(transfer.number)}`}
        subtitle="Accepted units go on hand at the destination; lost ones were broken or missing on the way."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <VStack gap={3}>
          {waiting.map((line) => {
            const c = counts[line.variant] ?? { accepted: 0, rejected: 0 };
            return (
              <HStack key={line.variant} gap={3} vAlign="end" hAlign="between" wrap="wrap">
                <VStack gap={0}>
                  <Text weight="medium">{transferLineLabel(line)}</Text>
                  <Text type="supporting" color="secondary">
                    {line.in_transit} of {line.shipped} on the way
                  </Text>
                </VStack>
                <HStack gap={2}>
                  <NumberInput
                    label="Accepted"
                    min={0}
                    value={c.accepted}
                    onChange={(accepted) => set(line.variant, { accepted })}
                  />
                  <NumberInput
                    label="Lost"
                    min={0}
                    value={c.rejected}
                    onChange={(rejected) => set(line.variant, { rejected })}
                  />
                </HStack>
              </HStack>
            );
          })}
        </VStack>
        {over.length > 0 && (
          <Banner
            status="error"
            title="More than is on its way"
            description={`${over.map(transferLineLabel).join(", ")}: no more can arrive than left.`}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Receive"
            variant="primary"
            isLoading={isPending}
            isDisabled={lines.length === 0 || over.length > 0}
            onClick={() =>
              startTransition(async () => {
                const result = await receiveTransfer(transfer.id, { key, lines });
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
