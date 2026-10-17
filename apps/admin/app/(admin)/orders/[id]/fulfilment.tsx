"use client";

// Shipping an order, and taking it back (design note 0018): the fulfil dialog, the
// shipments card and the cancel dialog of the order page.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ShipmentBadge } from "@/components/status";
import { CANCEL_REASONS } from "@/lib/format";

import { MAX_TRACKING, NO_TRACKING, type TrackingInput, trackingProblem } from "@/lib/tracking";

import { cancelOrder, fulfilOrder, takeBack, updateTracking } from "../actions";

type Order = AdminSchemas["AdminOrderResponse"];
type Line = AdminSchemas["OrderLineResponse"];
type Fulfillment = AdminSchemas["FulfillmentResponse"];
export type Place = { id: string; name: string };

function lineName(line: Line): string {
  return [line.product_title, line.variant_title].filter(Boolean).join(" · ");
}

/** What's left of each line to ship from `location`: units waiting there, plus units
 * waiting nowhere in particular (untracked items). */
function waitingAt(order: Order, location: string | undefined): Record<string, number> {
  const quantities: Record<string, number> = {};
  for (const left of order.unfulfilled) {
    if (left.location === location || left.location === null) {
      quantities[left.line] = (quantities[left.line] ?? 0) + left.quantity;
    }
  }
  return quantities;
}

/** Every tracking number of a shipment: edit, remove, add up to `MAX_TRACKING`. */
function TrackingList({
  rows,
  onChange,
}: {
  rows: TrackingInput[];
  onChange: (rows: TrackingInput[]) => void;
}) {
  const set = (index: number, row: TrackingInput) =>
    onChange(rows.map((old, i) => (i === index ? row : old)));
  return (
    <VStack gap={3}>
      {rows.map((row, index) => (
        // Rows have no ID of their own; their place in the list is what they are.
        <VStack key={index} gap={2}>
          <HStack gap={3} wrap="wrap" vAlign="end">
            <TextInput
              label="Carrier"
              isOptional
              placeholder="DHL"
              value={row.carrier}
              onChange={(carrier) => set(index, { ...row, carrier })}
            />
            <TextInput
              label="Tracking number"
              value={row.number}
              onChange={(number) => set(index, { ...row, number })}
            />
            {rows.length > 1 && (
              <Button
                label="Remove"
                size="sm"
                variant="ghost"
                onClick={() => onChange(rows.filter((_, i) => i !== index))}
              />
            )}
          </HStack>
          <TextInput
            label="Tracking link"
            isOptional
            placeholder="https://"
            value={row.url}
            onChange={(url) => set(index, { ...row, url })}
          />
        </VStack>
      ))}
      {rows.length < MAX_TRACKING && (
        <HStack>
          <Button
            label="Add another tracking number"
            size="sm"
            variant="ghost"
            onClick={() => onChange([...rows, NO_TRACKING])}
          />
        </HStack>
      )}
    </VStack>
  );
}

/** "Fulfil items": where they leave from, how many of each, and the tracking. */
export function FulfilButton({ order, places }: { order: Order; places: Place[] }) {
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button label="Fulfil items" variant="primary" onClick={() => setIsOpen(true)} />
      {isOpen && (
        <FulfilDialog order={order} places={places} onClose={() => setIsOpen(false)} />
      )}
    </>
  );
}

function FulfilDialog({
  order,
  places,
  onClose,
}: {
  order: Order;
  places: Place[];
  onClose: () => void;
}) {
  // Where most of it waits first; any location will do for untracked items.
  const first = order.unfulfilled.find((left) => left.location)?.location ?? places[0]?.id;
  const [location, setLocation] = useState(first);
  const [quantities, setQuantities] = useState(() => waitingAt(order, first));
  const [tracking, setTracking] = useState<TrackingInput[]>([NO_TRACKING]);
  const [notify, setNotify] = useState(true);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // One key per dialog: a double submit records one shipment.
  const [key] = useState(() => crypto.randomUUID());

  const open = order.lines.filter((line) => line.fulfilled_quantity < line.quantity);
  const lines = open
    .map((line) => ({ line: line.id, quantity: quantities[line.id] ?? 0 }))
    .filter((line) => line.quantity > 0);

  const submit = () => {
    if (!location || lines.length === 0) return;
    const problem = trackingProblem(tracking);
    if (problem) return setError(problem);
    startTransition(async () => {
      const result = await fulfilOrder(order.id, { key, location, lines, tracking, notify });
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={600} purpose="form">
      <DialogHeader
        title={`Fulfil order #${order.number}`}
        subtitle="Record what left, and from where. Its stock there goes down."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label="Ships from"
          options={places.map((place) => ({ value: place.id, label: place.name }))}
          value={location}
          onChange={(next) => {
            setLocation(next);
            setQuantities(waitingAt(order, next));
          }}
        />
        <VStack gap={2}>
          {open.map((line) => {
            const left = line.quantity - line.fulfilled_quantity;
            return (
              <HStack key={line.id} gap={3} vAlign="center" hAlign="between">
                <VStack gap={0}>
                  <Text weight="medium">{lineName(line)}</Text>
                  <Text type="supporting" color="secondary">
                    {left} of {line.quantity} left to ship
                  </Text>
                </VStack>
                <NumberInput
                  label={`Quantity of ${lineName(line)}`}
                  isLabelHidden
                  min={0}
                  max={left}
                  value={quantities[line.id] ?? 0}
                  onChange={(quantity) => setQuantities({ ...quantities, [line.id]: quantity })}
                />
              </HStack>
            );
          })}
        </VStack>
        <TrackingList rows={tracking} onChange={setTracking} />
        <CheckboxInput
          label="Email the customer"
          description="A shipping confirmation with the tracking."
          value={notify}
          onChange={setNotify}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Fulfil"
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

/** Each shipment: from where, what, its tracking; edit the tracking or take it back. */
export function ShipmentsCard({
  order,
  fulfillments,
  places,
}: {
  order: Order;
  fulfillments: Fulfillment[];
  places: Place[];
}) {
  const nameOf = (id: string) => places.find((place) => place.id === id)?.name ?? id;
  const lineOf = (id: string) => order.lines.find((line) => line.id === id);
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          Shipments
        </Text>
        {fulfillments.map((fulfillment) => (
          <Shipment
            key={fulfillment.id}
            orderId={order.id}
            fulfillment={fulfillment}
            from={nameOf(fulfillment.location)}
            describe={(line) => {
              const found = lineOf(line);
              return found ? lineName(found) : line;
            }}
          />
        ))}
      </VStack>
    </Card>
  );
}

function Shipment({
  orderId,
  fulfillment,
  from,
  describe,
}: {
  orderId: string;
  fulfillment: Fulfillment;
  from: string;
  describe: (line: string) => string;
}) {
  const [editing, setEditing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const stands = fulfillment.status === "shipped";

  return (
    <VStack gap={2}>
      <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
        <HStack gap={2} vAlign="center">
          <ShipmentBadge status={fulfillment.status} />
          <Text type="supporting" color="secondary">
            From {from}, <DateTime value={fulfillment.created_at} />
          </Text>
        </HStack>
        {stands &&
          (confirming ? (
            <HStack gap={1} vAlign="center">
              <Text type="supporting" color="secondary">
                Its items go back in stock.
              </Text>
              <Button label="Keep" size="sm" variant="ghost" onClick={() => setConfirming(false)} />
              <Button
                label="Take back"
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    setError((await takeBack(orderId, fulfillment.id)).error);
                    setConfirming(false);
                  })
                }
              />
            </HStack>
          ) : (
            <HStack gap={1}>
              <Button label="Edit tracking" size="sm" variant="ghost" onClick={() => setEditing(true)} />
              <Button label="Take back" size="sm" variant="ghost" onClick={() => setConfirming(true)} />
            </HStack>
          ))}
      </HStack>
      {error && <Banner status="error" title={error} />}
      <Text>
        {fulfillment.lines
          .map((line) => `${line.quantity} × ${describe(line.line)}`)
          .join(", ")}
      </Text>
      {fulfillment.tracking.map((t) => (
        <Text key={t.number} type="supporting">
          {t.carrier ? `${t.carrier} ` : ""}
          {t.url ? <Link href={t.url}>{t.number}</Link> : t.number}
        </Text>
      ))}
      {editing && (
        <TrackingDialog
          orderId={orderId}
          fulfillment={fulfillment}
          onClose={() => setEditing(false)}
        />
      )}
    </VStack>
  );
}

function TrackingDialog({
  orderId,
  fulfillment,
  onClose,
}: {
  orderId: string;
  fulfillment: Fulfillment;
  onClose: () => void;
}) {
  // Every number it has: saving replaces them all, so none may be left out.
  const [tracking, setTracking] = useState<TrackingInput[]>(() =>
    fulfillment.tracking.length === 0
      ? [NO_TRACKING]
      : fulfillment.tracking.map((t) => ({
          carrier: t.carrier ?? "",
          number: t.number,
          url: t.url ?? "",
        })),
  );
  const [notify, setNotify] = useState(true);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const submit = () => {
    const problem = trackingProblem(tracking);
    if (problem) return setError(problem);
    startTransition(async () => {
      const result = await updateTracking(orderId, fulfillment.id, tracking, notify);
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title="Edit tracking"
        subtitle="Clear a row, or remove it, to drop that number."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TrackingList rows={tracking} onChange={setTracking} />
        <CheckboxInput
          label="Email the customer"
          description="Only if the tracking changed."
          value={notify}
          onChange={setNotify}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Save" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}

const REASONS = Object.entries(CANCEL_REASONS).map(([value, label]) => ({ value, label }));

/** "Cancel order": refused while shipments stand, so it says so instead. */
export function CancelOrderButton({ order }: { order: Order }) {
  const [isOpen, setIsOpen] = useState(false);
  const shipped = order.fulfillment_status !== "unfulfilled";
  return (
    <>
      <Button
        label="Cancel order"
        variant="ghost"
        isDisabled={shipped}
        tooltip={shipped ? "Take back its shipments first." : undefined}
        onClick={() => setIsOpen(true)}
      />
      {isOpen && <CancelOrderDialog order={order} onClose={() => setIsOpen(false)} />}
    </>
  );
}

function CancelOrderDialog({ order, onClose }: { order: Order; onClose: () => void }) {
  const [reason, setReason] = useState("customer");
  const [notify, setNotify] = useState(true);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={520} purpose="form">
      <DialogHeader
        title={`Cancel order #${order.number}?`}
        subtitle={
          order.financial_status === "paid"
            ? "Its stock is released. It stays paid: refund the customer outside Hypha."
            : "Its stock is released. This can't be undone."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector label="Reason" options={REASONS} value={reason} onChange={setReason} />
        <CheckboxInput label="Email the customer" value={notify} onChange={setNotify} />
        <HStack gap={2} hAlign="end">
          <Button label="Keep order" variant="ghost" onClick={onClose} />
          <Button
            label="Cancel order"
            variant="destructive"
            isLoading={isPending}
            onClick={() =>
              startTransition(async () => {
                const result = await cancelOrder(order.id, reason, notify);
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
