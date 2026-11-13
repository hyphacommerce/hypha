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
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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
  const t = useT<"transfers">();
  const format = useFormat();
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
            {t("transfers.lines_title")}
          </Text>
          {canEdit && (
            <Button
              label={t("transfers.add_line")}
              variant="secondary"
              size="sm"
              onClick={() => setIsPicking(true)}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} />}
        {transfer.lines.length === 0 ? (
          <Text type="supporting" color="secondary">
            {t("transfers.lines_empty")}
          </Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("common.variant")}</TableHeaderCell>
                <TableHeaderCell>{t("common.quantity")}</TableHeaderCell>
                {shipped && <TableHeaderCell>{t("transfers.lines_received")}</TableHeaderCell>}
                {shipped && <TableHeaderCell>{t("transfers.lost")}</TableHeaderCell>}
                {shipped && <TableHeaderCell>{t("transfers.lines_in_transit")}</TableHeaderCell>}
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
                        {l.sku ? t("transfers.line_sku", { sku: l.sku }) : t("transfers.no_sku")}
                      </Text>
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Text>{format.number(l.quantity)}</Text>
                  </TableCell>
                  {shipped && (
                    <TableCell>
                      <Text>{format.number(l.received)}</Text>
                    </TableCell>
                  )}
                  {shipped && (
                    <TableCell>
                      <Text color={l.rejected > 0 ? undefined : "secondary"}>
                        {format.number(l.rejected)}
                      </Text>
                    </TableCell>
                  )}
                  {shipped && (
                    <TableCell>
                      <Text>{format.number(l.in_transit)}</Text>
                    </TableCell>
                  )}
                  {canEdit && (
                    <TableCell>
                      <HStack gap={1} hAlign="end">
                        <IconButton
                          label={t("transfers.edit_named", { name: transferLineLabel(l) })}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => startEditing(l.variant, transferLineLabel(l))}
                        />
                        <IconButton
                          label={t("transfers.remove_named", { name: transferLineLabel(l) })}
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
          title={t("transfers.picker_title")}
          mode="single"
          confirmLabel={() => t("transfers.picker_confirm")}
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
        title={
          removing ? t("transfers.remove_line_title", { name: transferLineLabel(removing) }) : ""
        }
        description={t("transfers.remove_line_description")}
        actionLabel={t("common.remove")}
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
  const t = useT<"transfers">();
  const [chosen, setChosen] = useState<string>();
  const variant = product.variants.find((v) => v.id === chosen);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={product.title}
        subtitle={t("transfers.choice_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label={t("common.variant")}
          options={product.variants.map((v) => {
            const name = [v.title ?? t("common.default"), v.sku].filter(Boolean).join(" · ");
            return {
              value: v.id,
              label: onTransfer.has(v.id)
                ? t("transfers.variant_on_transfer", { variant: name })
                : name,
            };
          })}
          value={chosen}
          onChange={setChosen}
          placeholder={t("transfers.choose_variant")}
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
  const t = useT<"transfers">();
  const [quantity, setQuantity] = useState(editing.quantity);
  const isValid = Number.isInteger(quantity) && quantity >= 1;
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={400} purpose="form">
      <DialogHeader
        title={editing.label}
        subtitle={t("transfers.quantity_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <NumberInput label={t("common.quantity")} min={1} value={quantity} onChange={setQuantity} />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
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
  const t = useT<"transfers">();
  const [isEditing, setIsEditing] = useState(false);
  const nameOf = (id: string) => places.find((p) => p.id === id)?.name ?? id;
  return (
    <Card>
      <VStack gap={3}>
        <HStack hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            {t("common.details")}
          </Text>
          {canEdit && <Link onClick={() => setIsEditing(true)}>{t("common.edit")}</Link>}
        </HStack>
        <MetadataList>
          <MetadataListItem label={t("transfers.from")}>
            <Text>{nameOf(transfer.origin)}</Text>
          </MetadataListItem>
          <MetadataListItem label={t("transfers.to")}>
            <Text>{nameOf(transfer.destination)}</Text>
          </MetadataListItem>
          <MetadataListItem label={t("transfers.expected")}>
            {transfer.expected_at ? (
              <CalendarDate value={transfer.expected_at} />
            ) : (
              <Text color="secondary">—</Text>
            )}
          </MetadataListItem>
          <MetadataListItem label={t("transfers.reference")}>
            <Text color={transfer.reference ? undefined : "secondary"}>
              {transfer.reference ?? "—"}
            </Text>
          </MetadataListItem>
          {transfer.shipped_at && (
            <MetadataListItem label={t("transfers.fact_shipped")}>
              <DateTime value={transfer.shipped_at} />
            </MetadataListItem>
          )}
          {transfer.received_at && (
            <MetadataListItem label={t("transfers.fact_received")}>
              <DateTime value={transfer.received_at} />
            </MetadataListItem>
          )}
          {transfer.cancelled_at && (
            <MetadataListItem label={t("transfers.fact_cancelled")}>
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
  const t = useT<"transfers">();
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
        title={t("transfers.details_dialog_title")}
        subtitle={t("transfers.details_dialog_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <div className="form-grid">
          <Selector
            label={t("transfers.from")}
            options={options}
            value={origin}
            onChange={setOrigin}
          />
          <Selector
            label={t("transfers.to")}
            options={options}
            value={destination}
            onChange={setDestination}
            status={same ? { type: "error", message: t("transfers.same_location") } : undefined}
          />
          <DateInput
            label={t("transfers.expected")}
            isOptional
            value={expected}
            hasClear
            weekStartsOn="mon"
            onChange={(value) => setExpected(value ?? undefined)}
          />
          <TextInput
            label={t("transfers.reference")}
            isOptional
            description={t("transfers.reference_description")}
            value={reference}
            onChange={setReference}
          />
        </div>
        <TextArea label={t("common.note")} isOptional value={note} onChange={setNote} />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
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
  const t = useT<"transfers">();
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
  const origin = places.find((p) => p.id === transfer.origin)?.name ?? t("transfers.the_origin");
  const number = trNumber(transfer.number);
  if (transfer.status !== "draft" && transfer.status !== "in_transit") return null;
  return (
    <VStack gap={2}>
      {error && <Banner status="error" title={error} />}
      <HStack gap={2} wrap="wrap">
        {transfer.status === "draft" ? (
          <>
            <Button
              label={t("common.delete")}
              variant="ghost"
              onClick={() => setConfirm("delete")}
            />
            <Button
              label={t("transfers.action_cancel")}
              variant="secondary"
              onClick={() => setConfirm("cancel")}
            />
            <Button
              label={t("transfers.ship")}
              variant="primary"
              isDisabled={transfer.lines.length === 0}
              onClick={() => setIsShipping(true)}
            />
          </>
        ) : (
          <Button
            label={t("transfers.receive")}
            variant="primary"
            onClick={() => setIsReceiving(true)}
          />
        )}
      </HStack>
      <AlertDialog
        isOpen={confirm !== undefined}
        onOpenChange={(open) => !open && setConfirm(undefined)}
        title={
          confirm === "delete"
            ? t("transfers.confirm_delete_title")
            : t("transfers.confirm_cancel_title")
        }
        description={
          confirm === "delete"
            ? t("transfers.confirm_delete_description", { number })
            : t("transfers.confirm_cancel_description", { number })
        }
        actionLabel={
          confirm === "delete" ? t("common.delete") : t("transfers.confirm_cancel_action")
        }
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
  const t = useT<"transfers">();
  const format = useFormat();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // One key per dialog: a double submit ships once.
  const [key] = useState(() => crypto.randomUUID());
  const short = transfer.lines.filter((l) => (atOrigin[l.variant] ?? 0) < l.quantity);
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={560} purpose="form">
      <DialogHeader
        title={t("transfers.ship_dialog_title", { number: trNumber(transfer.number) })}
        subtitle={t("transfers.ship_dialog_subtitle", { origin })}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <VStack gap={1}>
          {transfer.lines.map((l) => (
            <Text key={l.variant}>
              {t.rich("transfers.ship_line", {
                quantity: l.quantity,
                name: transferLineLabel(l),
                count: atOrigin[l.variant] ?? 0,
                origin,
                dim: (chunks, tag) => (
                  <Text key={tag} type="supporting" color="secondary">
                    {chunks}
                  </Text>
                ),
              })}
            </Text>
          ))}
        </VStack>
        {short.length > 0 && (
          <Banner
            status="warning"
            title={t("transfers.ship_short_title")}
            description={t("transfers.ship_short_description", {
              lines: format.list(short.map(transferLineLabel)),
            })}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={short.length > 0 ? t("transfers.ship_anyway") : t("transfers.ship_submit")}
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
  const t = useT<"transfers">();
  const format = useFormat();
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
        title={t("transfers.receive_dialog_title", { number: trNumber(transfer.number) })}
        subtitle={t("transfers.receive_dialog_subtitle")}
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
                    {t("transfers.receive_on_the_way", {
                      in_transit: line.in_transit,
                      shipped: line.shipped,
                    })}
                  </Text>
                </VStack>
                <HStack gap={2}>
                  <NumberInput
                    label={t("transfers.accepted")}
                    min={0}
                    value={c.accepted}
                    onChange={(accepted) => set(line.variant, { accepted })}
                  />
                  <NumberInput
                    label={t("transfers.lost")}
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
            title={t("transfers.receive_over_title")}
            description={t("transfers.receive_over_description", {
              lines: format.list(over.map(transferLineLabel)),
            })}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("transfers.receive_submit")}
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
