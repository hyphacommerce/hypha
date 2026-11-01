"use client";

// Goods arriving against a purchase order: the receive dialog and the receipts card of
// the purchase order page.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { DateTime } from "@/components/date-time";
import { formatUnitCost } from "@/lib/format";
import { type PurchaseOrder, lineLabel, poNumber } from "@/lib/purchasing";

import { cancelReceipt, receivePurchaseOrder, trackVariants } from "../actions";

type Receipt = AdminSchemas["ReceiptResponse"];
export type Place = { id: string; name: string };
type Counts = Record<string, { accepted: number; rejected: number }>;

/** What's outstanding on each line, as the dialog starts: all of it accepted. */
function outstanding(po: PurchaseOrder): Counts {
  return Object.fromEntries(
    po.lines
      .filter((l) => l.outstanding > 0)
      .map((l) => [l.variant, { accepted: l.outstanding, rejected: 0 }]),
  );
}

/** "Receive": where the goods came in, and how many of each were accepted or refused. */
export function ReceiveButton({ po, places }: { po: PurchaseOrder; places: Place[] }) {
  const [isOpen, setIsOpen] = useState(false);
  if (po.status !== "ordered") return null;
  return (
    <>
      <Button label="Receive" variant="primary" onClick={() => setIsOpen(true)} />
      {isOpen && <ReceiveDialog po={po} places={places} onClose={() => setIsOpen(false)} />}
    </>
  );
}

function ReceiveDialog({
  po,
  places,
  onClose,
}: {
  po: PurchaseOrder;
  places: Place[];
  onClose: () => void;
}) {
  const [location, setLocation] = useState(po.destination ?? places[0]?.id);
  const [counts, setCounts] = useState<Counts>(() => outstanding(po));
  const [note, setNote] = useState("");
  const [error, setError] = useState<string>();
  const [untracked, setUntracked] = useState<string[]>();
  const [isPending, startTransition] = useTransition();
  // One key per dialog: a double submit records one receipt.
  const [key] = useState(() => crypto.randomUUID());

  const lines = po.lines
    .map((l) => ({ variant: l.variant, ...(counts[l.variant] ?? { accepted: 0, rejected: 0 }) }))
    .filter((l) => l.accepted + l.rejected > 0);
  const over = po.lines.filter((l) => {
    const c = counts[l.variant];
    return c !== undefined && c.accepted + c.rejected > l.outstanding;
  });
  const set = (variant: string, change: Partial<{ accepted: number; rejected: number }>) =>
    setCounts({
      ...counts,
      [variant]: { ...(counts[variant] ?? { accepted: 0, rejected: 0 }), ...change },
    });

  const submit = () => {
    if (!location || lines.length === 0) return;
    startTransition(async () => {
      const result = await receivePurchaseOrder(po.id, {
        key,
        location,
        lines,
        allowOver: over.length > 0,
        note,
      });
      if (result.error) return setError(result.error);
      if (result.newItems && result.newItems.length > 0) setUntracked(result.newItems);
      else onClose();
    });
  };

  const nameOf = (variant: string) => {
    const line = po.lines.find((l) => l.variant === variant);
    return line ? lineLabel(line) : variant;
  };

  if (untracked) {
    return (
      <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560}>
        <DialogHeader
          title="Received"
          subtitle="Some of it had no stock record, so one was made, untracked: whether it sells didn't change."
          onOpenChange={(isOpen) => !isOpen && onClose()}
        />
        <VStack gap={4} padding={4}>
          {error && <Banner status="error" title={error} />}
          <Text>{untracked.map(nameOf).join(", ")}</Text>
          <Text type="supporting" color="secondary">
            Track their stock to sell only what's on hand.
          </Text>
          <HStack gap={2} hAlign="end">
            <Button label="Not now" variant="ghost" onClick={onClose} />
            <Button
              label="Track stock"
              variant="primary"
              isLoading={isPending}
              onClick={() =>
                startTransition(async () => {
                  const result = await trackVariants(untracked);
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

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={640} purpose="form">
      <DialogHeader
        title={`Receive ${poNumber(po.number)}`}
        subtitle="Accepted units go on hand at their landed cost; refused ones are only counted."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label="Arrived at"
          options={places.map((place) => ({ value: place.id, label: place.name }))}
          value={location}
          onChange={setLocation}
        />
        <VStack gap={3}>
          {po.lines.map((line) => {
            const c = counts[line.variant] ?? { accepted: 0, rejected: 0 };
            return (
              <HStack key={line.variant} gap={3} vAlign="end" hAlign="between" wrap="wrap">
                <VStack gap={0}>
                  <Text weight="medium">{lineLabel(line)}</Text>
                  <Text type="supporting" color="secondary">
                    {line.outstanding} of {line.quantity} still to come
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
                    label="Refused"
                    min={0}
                    value={c.rejected}
                    onChange={(rejected) => set(line.variant, { rejected })}
                  />
                </HStack>
              </HStack>
            );
          })}
        </VStack>
        <TextInput
          label="Note"
          isOptional
          placeholder="Pallet 1 of 2"
          value={note}
          onChange={setNote}
        />
        {over.length > 0 && (
          <Banner
            status="warning"
            title="More than was ordered"
            description={`${over.map((l) => lineLabel(l)).join(", ")}: the supplier sent more than is still to come. Receive it anyway?`}
          />
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={over.length > 0 ? "Receive anyway" : "Receive"}
            variant="primary"
            isLoading={isPending}
            isDisabled={!location || lines.length === 0}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** Each receipt: where and when, what came in at what cost; take one back. */
export function ReceiptsCard({
  po,
  receipts,
  places,
  canWrite,
}: {
  po: PurchaseOrder;
  receipts: Receipt[];
  places: Place[];
  canWrite: boolean;
}) {
  const nameOf = (id: string) => places.find((place) => place.id === id)?.name ?? id;
  if (receipts.length === 0) return null;
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Receipts
        </Text>
        {receipts.map((receipt) => (
          <ReceiptRow
            key={receipt.id}
            po={po}
            receipt={receipt}
            at={nameOf(receipt.location)}
            canWrite={canWrite && po.status !== "cancelled"}
          />
        ))}
      </VStack>
    </Card>
  );
}

function ReceiptRow({
  po,
  receipt,
  at,
  canWrite,
}: {
  po: PurchaseOrder;
  receipt: Receipt;
  at: string;
  canWrite: boolean;
}) {
  const currencies = useCurrencies();
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const stands = receipt.status === "recorded";
  const label = (variant: string) => {
    const line = po.lines.find((l) => l.variant === variant);
    return line ? lineLabel(line) : variant;
  };

  return (
    <VStack gap={2}>
      <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
        <Text type="supporting" color="secondary">
          {stands ? "" : "Taken back · "}At {at}, <DateTime value={receipt.created_at} />
        </Text>
        {stands &&
          canWrite &&
          (confirming ? (
            <HStack gap={1} vAlign="center">
              <Text type="supporting" color="secondary">
                Its units come off stock.
              </Text>
              <Button label="Keep" size="sm" variant="ghost" onClick={() => setConfirming(false)} />
              <Button
                label="Take back"
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    setError((await cancelReceipt(po.id, receipt.id)).error);
                    setConfirming(false);
                  })
                }
              />
            </HStack>
          ) : (
            <Button
              label="Take back"
              size="sm"
              variant="ghost"
              onClick={() => setConfirming(true)}
            />
          ))}
      </HStack>
      {error && <Banner status="error" title={error} />}
      {receipt.lines.map((line) => (
        <Text key={line.variant} color={stands ? undefined : "secondary"}>
          {line.accepted} × {label(line.variant)}
          {line.rejected > 0 ? `, ${line.rejected} refused` : ""}
          {line.landed_unit_cost && line.accepted > 0
            ? ` · ${formatUnitCost(line.landed_unit_cost, currencies)} each`
            : ""}
        </Text>
      ))}
      {receipt.note && (
        <Text type="supporting" color="secondary">
          {receipt.note}
        </Text>
      )}
    </VStack>
  );
}
