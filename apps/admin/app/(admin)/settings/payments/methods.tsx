"use client";

// Payment methods: the list in the order shoppers see it, adding one from a preset,
// editing, switching on and off, moving up and down, and deleting.

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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState, useTransition } from "react";

import {
  type PaymentApp,
  type PaymentNameDefaults,
  type PaymentPreset,
  paymentPresets,
  providerLabel,
} from "@/lib/payments";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

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
function where(
  ids: string[],
  options: Option[],
  every: string,
  list: (names: string[]) => string,
): string {
  if (ids.length === 0) return every;
  const names = new Map(options.map((o) => [o.value, o.label]));
  return list(ids.map((id) => names.get(id) ?? id));
}

export function PaymentMethodsCard({
  methods,
  markets,
  channels,
  paymentApps,
  presetNames,
  canWrite,
}: {
  /** By position, then ID: as the API lists them. */
  methods: Method[];
  /** The names a new method starts with, in the store's default language. */
  presetNames: PaymentNameDefaults;
  markets: Option[];
  channels: Option[];
  paymentApps: PaymentApp[];
  canWrite: boolean;
}) {
  const t = useT<"payments">();
  const format = useFormat();
  const list = (names: string[]) => format.list(names, "unit");
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
              {t("payments.methods_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("payments.methods_description")}
            </Text>
          </VStack>
          {canWrite && (
            <Button
              label={t("payments.methods_add")}
              variant="secondary"
              onClick={() => setEditing("new")}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {methods.length === 0 ? (
          <EmptyState
            title={t("payments.methods_empty_title")}
            description={t("payments.methods_empty_description")}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("payments.methods_column_method")}</TableHeaderCell>
                <TableHeaderCell>{t("payments.methods_column_offered_in")}</TableHeaderCell>
                <TableHeaderCell>{t("common.enabled")}</TableHeaderCell>
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
                        <Badge label={providerLabel(method.provider, t)} />
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
                        {where(method.markets, markets, t("payments.methods_every_market"), list)}
                      </Text>
                      <Text type="supporting" color="secondary">
                        {where(
                          method.channels,
                          channels,
                          t("payments.methods_every_channel"),
                          list,
                        )}
                      </Text>
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <Switch
                      label={t("payments.method_offer", { name: method.name })}
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
                          label={t("common.move_up_named", { name: method.name })}
                          icon={<ArrowUpIcon />}
                          size="sm"
                          variant="ghost"
                          isDisabled={index === 0 || isMoving}
                          onClick={() => move(index, -1)}
                        />
                        <IconButton
                          label={t("common.move_down_named", { name: method.name })}
                          icon={<ArrowDownIcon />}
                          size="sm"
                          variant="ghost"
                          isDisabled={index === methods.length - 1 || isMoving}
                          onClick={() => move(index, 1)}
                        />
                        <IconButton
                          label={t("common.edit_named", { name: method.name })}
                          icon={<PencilSquareIcon />}
                          size="sm"
                          variant="ghost"
                          onClick={() => setEditing(method)}
                        />
                        <IconButton
                          label={t("common.delete_named_label", { name: method.name })}
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
          presetNames={presetNames}
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
function kindOf(value: string, presets: PaymentPreset[], paymentApps: PaymentApp[]): Kind {
  const app = value.startsWith("app:")
    ? paymentApps.find((a) => `app:${a.key}` === value)
    : undefined;
  if (app) return { app };
  return { preset: presets.find((p) => p.value === value) ?? presets[0] };
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
  presetNames,
  onClose,
}: {
  method?: Method;
  markets: Option[];
  channels: Option[];
  paymentApps: PaymentApp[];
  presetNames: PaymentNameDefaults;
  onClose: () => void;
}) {
  const t = useT<"payments">();
  const presets = paymentPresets(t, presetNames);
  const [kind, setKind] = useState<Kind>({ preset: presets[0] });
  const kindName = kind.app ? kind.app.name : kind.preset.name;
  const kindValue = kind.app ? `app:${kind.app.key}` : kind.preset.value;
  // An app's or spore's method collects the money itself: no instructions to show.
  const isApp = method ? method.provider !== "manual" : Boolean(kind.app);
  const [form, setForm] = useState<MethodInput>({
    name: method?.name ?? presets[0].name,
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
    const next = kindOf(value, presets, paymentApps);
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
        title={
          method
            ? t("payments.method_dialog_title_edit", { name: method.name })
            : t("payments.method_dialog_title_add")
        }
        subtitle={
          method
            ? t("payments.method_dialog_subtitle_edit")
            : kind.app
              ? t("payments.method_dialog_subtitle_app", { app: kind.app.name })
              : t("payments.method_dialog_subtitle_manual")
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The fields scroll on short screens; the buttons stay in view below them. A flex
          item keeps its content's height unless allowed to shrink (`minHeight: 0`). */}
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          {!method && (
            <Selector
              label={t("common.kind")}
              options={[
                ...presets.map(({ value, label, description }) => ({
                  value,
                  label,
                  description,
                })),
                ...paymentApps.map((app) => ({
                  value: `app:${app.key}`,
                  label: app.name,
                  description: app.isBuiltIn
                    ? t("payments.method_kind_built_in", { app: app.name })
                    : t("payments.method_kind_app", { app: app.name }),
                })),
              ]}
              value={kindValue}
              onChange={chooseKind}
            />
          )}
          <TextInput
            label={t("common.name")}
            description={t("payments.method_name_description")}
            placeholder={t("payments.method_name_placeholder")}
            value={form.name}
            onChange={(name) => set({ name })}
          />
          {!isApp && (
            <TextArea
              label={t("payments.method_instructions_label")}
              description={t("payments.method_instructions_description")}
              placeholder={method || kind.app ? undefined : kind.preset.instructionsPlaceholder}
              isOptional
              maxLength={2000}
              rows={4}
              value={form.instructions}
              onChange={(instructions) => set({ instructions })}
            />
          )}
          <MultiSelector
            label={t("common.markets")}
            description={t("payments.method_markets_description")}
            placeholder={t("common.every_market")}
            options={markets}
            value={form.markets}
            onChange={(chosen) => set({ markets: chosen })}
            hasSearch={markets.length > 15}
            triggerDisplay="labels"
          />
          <MultiSelector
            label={t("common.channels")}
            description={t("payments.method_channels_description")}
            placeholder={t("common.every_channel")}
            options={channels}
            value={form.channels}
            onChange={(chosen) => set({ channels: chosen })}
            triggerDisplay="labels"
          />
          <Switch
            label={t("common.enabled")}
            description={t("payments.method_enabled_description")}
            value={form.enabled}
            onChange={(enabled) => set({ enabled })}
          />
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={method ? t("common.save") : t("payments.method_add_submit")}
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
  const t = useT<"payments">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("common.delete_named", { name: method.name })}
      description={error ?? t("payments.method_delete_description")}
      actionLabel={t("payments.method_delete_action")}
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
