"use client";

// Payment methods (design note 0027): the list in the order shoppers see it, adding one
// from a preset, editing, switching on and off, moving up and down, and deleting.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
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
import {
  ArrowDownIcon,
  ArrowUpIcon,
  PencilSquareIcon,
  TrashIcon,
} from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState, useTransition } from "react";

import {
  PAYMENT_PRESETS,
  type PaymentApp,
  type PaymentPreset,
  providerLabel,
} from "@/lib/payments";

import {
  createPaymentMethod,
  deletePaymentMethod,
  type MethodInput,
  reorderPaymentMethods,
  setPaymentMethodEnabled,
  updatePaymentMethod,
} from "./actions";

type Method = AdminSchemas["PaymentMethodResponse"];
type Option = { value: string; label: string };

/** "Every market", or the names of the ones listed (IDs for any this admin can't name). */
function where(ids: string[], options: Option[], every: string): string {
  if (ids.length === 0) return every;
  const names = new Map(options.map((o) => [o.value, o.label]));
  return ids.map((id) => names.get(id) ?? id).join(", ");
}

export function PaymentMethodsCard({
  methods,
  markets,
  channels,
  paymentApps,
  canWrite,
}: {
  /** By position, then ID: as the API lists them. */
  methods: Method[];
  markets: Option[];
  channels: Option[];
  paymentApps: PaymentApp[];
  canWrite: boolean;
}) {
  const [editing, setEditing] = useState<Method | "new">();
  const [deleting, setDeleting] = useState<Method>();
  const [error, setError] = useState<string>();
  const [isMoving, startMoving] = useTransition();

  const move = (index: number, by: -1 | 1) =>
    startMoving(async () => {
      const order = [...methods];
      [order[index], order[index + by]] = [order[index + by], order[index]];
      setError(
        (await reorderPaymentMethods(order.map(({ id, position }) => ({ id, position })))).error,
      );
    });

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Payment methods
            </Text>
            <Text type="supporting" color="secondary">
              Shoppers see the enabled ones offered in their market and channel, in this order.
            </Text>
          </VStack>
          {canWrite && (
            <Button
              label="Add payment method"
              variant="secondary"
              onClick={() => setEditing("new")}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {methods.length === 0 ? (
          <EmptyState
            title="No payment methods yet"
            description="Add cash on delivery or a bank transfer to start taking orders."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Method</TableHeaderCell>
                <TableHeaderCell>Offered in</TableHeaderCell>
                <TableHeaderCell>Enabled</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {methods.map((method, index) => (
                <TableRow key={method.id}>
                  <TableCell>
                    <VStack gap={0.5}>
                      <HStack gap={2} vAlign="center">
                        <Text weight="medium">{method.name}</Text>
                        <Badge label={providerLabel(method.provider)} />
                      </HStack>
                      {method.instructions && (
                        <Text type="supporting" color="secondary">
                          {method.instructions.length > 120
                            ? `${method.instructions.slice(0, 120)}…`
                            : method.instructions}
                        </Text>
                      )}
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <VStack gap={0.5}>
                      <Text type="supporting">
                        {where(method.markets, markets, "Every market")}
                      </Text>
                      <Text type="supporting" color="secondary">
                        {where(method.channels, channels, "Every channel")}
                      </Text>
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Switch
                      label={`Offer ${method.name}`}
                      isLabelHidden
                      size="sm"
                      value={method.enabled}
                      isDisabled={!canWrite}
                      changeAction={async (enabled) =>
                        setError((await setPaymentMethodEnabled(method.id, enabled)).error)
                      }
                    />
                  </TableCell>
                  <TableCell>
                    {canWrite && (
                      <HStack gap={1} hAlign="end">
                        <IconButton
                          label={`Move ${method.name} up`}
                          icon={<ArrowUpIcon />}
                          size="sm"
                          variant="ghost"
                          isDisabled={index === 0 || isMoving}
                          onClick={() => move(index, -1)}
                        />
                        <IconButton
                          label={`Move ${method.name} down`}
                          icon={<ArrowDownIcon />}
                          size="sm"
                          variant="ghost"
                          isDisabled={index === methods.length - 1 || isMoving}
                          onClick={() => move(index, 1)}
                        />
                        <IconButton
                          label={`Edit ${method.name}`}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setEditing(method)}
                        />
                        <IconButton
                          label={`Delete ${method.name}`}
                          icon={<TrashIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setDeleting(method)}
                        />
                      </HStack>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {editing && (
        <MethodDialog
          method={editing === "new" ? undefined : editing}
          markets={markets}
          channels={channels}
          paymentApps={paymentApps}
          onClose={() => setEditing(undefined)}
        />
      )}
      {deleting && <DeleteDialog method={deleting} onClose={() => setDeleting(undefined)} />}
    </Card>
  );
}

/** What a new method can be: a manual preset, or an installed payment app (`app:<key>`). */
type Kind = { preset: PaymentPreset; app?: undefined } | { app: PaymentApp; preset?: undefined };

/** The preset or app a kind's value names. */
function kindOf(value: string, paymentApps: PaymentApp[]): Kind {
  const app = value.startsWith("app:")
    ? paymentApps.find((a) => `app:${a.key}` === value)
    : undefined;
  if (app) return { app };
  return { preset: PAYMENT_PRESETS.find((p) => p.value === value) ?? PAYMENT_PRESETS[0] };
}

/**
 * Adds a method, manual from a preset or taken by an installed payment app, or edits one
 * (`method`): its provider can't change.
 */
function MethodDialog({
  method,
  markets,
  channels,
  paymentApps,
  onClose,
}: {
  method?: Method;
  markets: Option[];
  channels: Option[];
  paymentApps: PaymentApp[];
  onClose: () => void;
}) {
  const [kind, setKind] = useState<Kind>({ preset: PAYMENT_PRESETS[0] });
  const kindName = kind.app ? kind.app.name : kind.preset.name;
  const kindValue = kind.app ? `app:${kind.app.key}` : kind.preset.value;
  // An app's method collects the money itself: no instructions to show the shopper.
  const isApp = method ? method.provider.startsWith("app_") : Boolean(kind.app);
  const [form, setForm] = useState<MethodInput>({
    name: method?.name ?? PAYMENT_PRESETS[0].name,
    instructions: method?.instructions ?? "",
    markets: method?.markets ?? [],
    channels: method?.channels ?? [],
    enabled: method?.enabled ?? true,
  });
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const set = (change: Partial<MethodInput>) => setForm((f) => ({ ...f, ...change }));
  const isComplete = form.name.trim() !== "" && form.instructions.length <= 2000;

  const chooseKind = (value: string) => {
    const next = kindOf(value, paymentApps);
    // The name follows the kind unless staff typed their own.
    if (form.name === kindName) set({ name: next.app ? next.app.name : next.preset.name });
    setKind(next);
  };

  const submit = () => {
    if (!isComplete) return;
    startTransition(async () => {
      const result = method
        ? await updatePaymentMethod(method.id, form)
        : await createPaymentMethod({
            ...form,
            provider: kind.app ? kind.app.key : "manual",
            instructions: kind.app ? "" : form.instructions,
          });
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={560} purpose="form">
      <DialogHeader
        title={method ? `Edit ${method.name}` : "Add payment method"}
        subtitle={
          method
            ? "Orders already placed keep the name and instructions they were placed with."
            : kind.app
              ? `The shopper pays at checkout through ${kind.app.name}; the app reports the payment.`
              : "A manual method: the shopper pays you directly, and you mark the order paid."
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The fields scroll on short screens; the buttons stay in view below them. A flex
          item keeps its content's height unless allowed to shrink (`minHeight: 0`). */}
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          {!method && (
            <Selector
              label="Kind"
              options={[
                ...PAYMENT_PRESETS.map(({ value, label, description }) => ({
                  value,
                  label,
                  description,
                })),
                ...paymentApps.map((app) => ({
                  value: `app:${app.key}`,
                  label: app.name,
                  description: `Taken by the ${app.name} app: the shopper pays at checkout.`,
                })),
              ]}
              value={kindValue}
              onChange={chooseKind}
            />
          )}
          <TextInput
            label="Name"
            description="What shoppers see at checkout."
            placeholder="Cash on delivery"
            value={form.name}
            onChange={(name) => set({ name })}
          />
          {!isApp && (
            <TextArea
              label="Instructions"
              description="Shown to the shopper after checkout and on their order: bank details, what to pay the courier."
              placeholder={method || kind.app ? undefined : kind.preset.instructionsPlaceholder}
              isOptional
              maxLength={2000}
              rows={4}
              value={form.instructions}
              onChange={(instructions) => set({ instructions })}
            />
          )}
          <MultiSelector
            label="Markets"
            description="None chosen: offered in every market."
            placeholder="Every market"
            options={markets}
            value={form.markets}
            onChange={(chosen) => set({ markets: chosen })}
            hasSearch={markets.length > 15}
            triggerDisplay="labels"
          />
          <MultiSelector
            label="Channels"
            description="None chosen: offered in every channel."
            placeholder="Every channel"
            options={channels}
            value={form.channels}
            onChange={(chosen) => set({ channels: chosen })}
            triggerDisplay="labels"
          />
          <Switch
            label="Enabled"
            description="Shoppers see it at checkout."
            value={form.enabled}
            onChange={(enabled) => set({ enabled })}
          />
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={method ? "Save" : "Add method"}
            variant="primary"
            isLoading={isPending}
            isDisabled={!isComplete}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function DeleteDialog({ method, onClose }: { method: Method; onClose: () => void }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={`Delete ${method.name}?`}
      description={
        error ??
        "Shoppers stop seeing it. Orders already paid this way keep its name and instructions."
      }
      actionLabel="Delete method"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await deletePaymentMethod(method.id);
          if (result.error) setError(result.error);
          else onClose();
        })
      }
    />
  );
}
