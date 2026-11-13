"use client";

// Shipping an order, and taking it back: the fulfil dialog, the shipments card and
// the cancel dialog of the order page.

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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { ShipmentBadge } from "@/components/status";
import { CANCEL_REASONS, cancelReasonLabel } from "@/lib/format";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";
import { COLLECTED } from "@/lib/payments";

import { MAX_TRACKING, NO_TRACKING, type TrackingInput, trackingProblem } from "@/lib/tracking";

import { cancelOrder, fulfilOrder, takeBack, updateTracking } from "../actions";

type Order = AdminSchemas["AdminOrderResponse"];
type Line = AdminSchemas["OrderLineResponse"];
type Fulfillment = AdminSchemas["FulfillmentResponse"];
export type Place = { id: string; name: string };

/** Examples in the tracking fields: a carrier's name and a link's start, the same in every language. */
const CARRIER_EXAMPLE = "DHL";
const LINK_EXAMPLE = "https://";

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
  const t = useT<"orders">();
  const set = (index: number, row: TrackingInput) =>
    onChange(rows.map((old, i) => (i === index ? row : old)));
  return (
    <VStack gap={3}>
      {rows.map((row, index) => (
        // Rows have no ID of their own; their place in the list is what they are, and
        // every field is controlled, so a removed row can't leave its text behind.
        // oxlint-disable-next-line react/no-array-index-key
        <VStack key={index} gap={2}>
          <HStack gap={3} wrap="wrap" vAlign="end">
            <TextInput
              label={t("orders.tracking_carrier")}
              isOptional
              placeholder={CARRIER_EXAMPLE}
              value={row.carrier}
              onChange={(carrier) => set(index, { ...row, carrier })}
            />
            <TextInput
              label={t("orders.tracking_number")}
              value={row.number}
              onChange={(number) => set(index, { ...row, number })}
            />
            {rows.length > 1 && (
              <Button
                label={t("common.remove")}
                size="sm"
                variant="ghost"
                onClick={() => onChange(rows.filter((_, i) => i !== index))}
              />
            )}
          </HStack>
          <TextInput
            label={t("orders.tracking_link")}
            isOptional
            placeholder={LINK_EXAMPLE}
            value={row.url}
            onChange={(url) => set(index, { ...row, url })}
          />
        </VStack>
      ))}
      {rows.length < MAX_TRACKING && (
        <HStack>
          <Button
            label={t("orders.tracking_add")}
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
  const t = useT<"orders">();
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <Button
        label={t("orders.fulfil_button")}
        variant="primary"
        onClick={() => setIsOpen(true)}
      />
      {isOpen && <FulfilDialog order={order} places={places} onClose={() => setIsOpen(false)} />}
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
  const t = useT<"orders">();
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
    const problem = trackingProblem(t, tracking);
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
        title={t("orders.fulfil_dialog_title", { number: String(order.number) })}
        subtitle={t("orders.fulfil_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label={t("orders.fulfil_ships_from")}
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
                    {t("orders.fulfil_line_left", { left, quantity: line.quantity })}
                  </Text>
                </VStack>
                <NumberInput
                  label={t("orders.fulfil_line_quantity", { name: lineName(line) })}
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
          label={t("orders.notify_customer")}
          description={t("orders.fulfil_notify_description")}
          value={notify}
          onChange={setNotify}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("orders.fulfil_submit")}
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
  const t = useT<"orders">();
  const nameOf = (id: string) => places.find((place) => place.id === id)?.name ?? id;
  const lineOf = (id: string) => order.lines.find((line) => line.id === id);
  return (
    <Card>
      <VStack gap={4}>
        <Text type="display-3" as="h2">
          {t("orders.shipments_title")}
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
  const t = useT<"orders">();
  const format = useFormat();
  const [editing, setEditing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const stands = fulfillment.status === "shipped";

  return (
    <VStack gap={2}>
      <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
        <HStack gap={2} vAlign="center">
          <ShipmentBadge status={fulfillment.status} t={t} />
          <Text type="supporting" color="secondary">
            {t.rich("orders.shipment_from", {
              location: from,
              time: <DateTime key="time" value={fulfillment.created_at} />,
            })}
          </Text>
        </HStack>
        {stands &&
          (confirming ? (
            <HStack gap={1} vAlign="center">
              <Text type="supporting" color="secondary">
                {t("orders.shipment_take_back_note")}
              </Text>
              <Button
                label={t("orders.shipment_keep")}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(false)}
              />
              <Button
                label={t("orders.shipment_take_back")}
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
              <Button
                label={t("orders.tracking_dialog_title")}
                size="sm"
                variant="ghost"
                onClick={() => setEditing(true)}
              />
              <Button
                label={t("orders.shipment_take_back")}
                size="sm"
                variant="ghost"
                onClick={() => setConfirming(true)}
              />
            </HStack>
          ))}
      </HStack>
      {error && <Banner status="error" title={error} />}
      <Text>
        {format.list(
          fulfillment.lines.map((line) =>
            t("orders.shipment_line", { quantity: line.quantity, name: describe(line.line) }),
          ),
          "unit",
        )}
      </Text>
      {fulfillment.tracking.map((tracking) => (
        <Text key={tracking.number} type="supporting">
          {tracking.carrier ? `${tracking.carrier} ` : ""}
          {tracking.url ? <Link href={tracking.url}>{tracking.number}</Link> : tracking.number}
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
      : fulfillment.tracking.map((row) => ({
          carrier: row.carrier ?? "",
          number: row.number,
          url: row.url ?? "",
        })),
  );
  const t = useT<"orders">();
  const [notify, setNotify] = useState(true);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const submit = () => {
    const problem = trackingProblem(t, tracking);
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
        title={t("orders.tracking_dialog_title")}
        subtitle={t("orders.tracking_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TrackingList rows={tracking} onChange={setTracking} />
        <CheckboxInput
          label={t("orders.notify_customer")}
          description={t("orders.tracking_notify_description")}
          value={notify}
          onChange={setNotify}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("common.save")}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** "Cancel order": refused while shipments stand, so it says so instead. */
export function CancelOrderButton({ order }: { order: Order }) {
  const t = useT<"orders">();
  const [isOpen, setIsOpen] = useState(false);
  const shipped = order.fulfillment_status !== "unfulfilled";
  return (
    <>
      <Button
        label={t("orders.cancel_button")}
        variant="ghost"
        isDisabled={shipped}
        tooltip={shipped ? t("orders.cancel_shipped_tooltip") : undefined}
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
  const t = useT<"orders">();
  const reasons = CANCEL_REASONS.map((value) => ({ value, label: cancelReasonLabel(t, value) }));
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={520} purpose="form">
      <DialogHeader
        title={t("orders.cancel_dialog_title", { number: String(order.number) })}
        subtitle={
          COLLECTED.includes(order.financial_status)
            ? t("orders.cancel_dialog_paid")
            : order.payments.some((p) => p.status === "pending")
              ? t("orders.cancel_dialog_pending")
              : t("orders.cancel_dialog_unpaid")
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <Selector
          label={t("orders.cancel_dialog_reason")}
          options={reasons}
          value={reason}
          onChange={setReason}
        />
        <CheckboxInput label={t("orders.notify_customer")} value={notify} onChange={setNotify} />
        <HStack gap={2} hAlign="end">
          <Button label={t("orders.cancel_keep")} variant="ghost" onClick={onClose} />
          <Button
            label={t("orders.cancel_button")}
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
