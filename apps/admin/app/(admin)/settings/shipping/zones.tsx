"use client";

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
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
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { ArrowDownIcon, ArrowUpIcon, PencilIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { currencyCodes } from "@/lib/format";
import {
  RATE_KINDS,
  type RateDraft,
  describeRate,
  emptyDraft,
  fromDraft,
  toDraft,
} from "@/lib/shipping-rates";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import {
  addOption,
  createZone,
  deleteZone,
  removeOption,
  reorderOptions,
  updateOption,
  updateZone,
} from "./actions";
import type { RateApp } from "./providers";

type Zone = AdminSchemas["ShippingZoneResponse"];
type Option = AdminSchemas["ShippingOptionResponse"];
type Provider = RateApp;
type Currency = AdminSchemas["Currency"];

export function ZonesSection({ zones, providers }: { zones: Zone[]; providers: Provider[] }) {
  const t = useT<"shipping">();
  const [isAdding, setIsAdding] = useState(false);

  return (
    <VStack gap={4}>
      <HStack gap={2} hAlign="between" vAlign="center">
        <Text type="display-3" as="h2">
          {t("shipping.zones_title")}
        </Text>
        <Button
          label={t("shipping.zones_add")}
          variant="secondary"
          onClick={() => setIsAdding(true)}
        />
      </HStack>
      {zones.length === 0 && (
        <Card>
          <EmptyState
            title={t("shipping.zones_empty_title")}
            description={t("shipping.zones_empty_description")}
          />
        </Card>
      )}
      {zones.map((zone) => (
        <ZoneCard key={zone.id} zone={zone} providers={providers} />
      ))}
      {isAdding && <ZoneDialog onClose={() => setIsAdding(false)} />}
    </VStack>
  );
}

/** A new zone, or `zone`'s name and countries. */
function ZoneDialog({ zone, onClose }: { zone?: Zone; onClose: () => void }) {
  const t = useT<"shipping">();
  const [name, setName] = useState(zone?.name ?? "");
  const [countries, setCountries] = useState(() => zone?.countries.join(", ") ?? "");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const submit = () => {
    if (!name.trim() || !countries.trim()) return;
    startTransition(async () => {
      const input = { name, countries };
      const result = zone ? await updateZone(zone.id, input) : await createZone(input);
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={zone ? t("shipping.zone_edit") : t("shipping.zone_dialog_title_add")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label={t("common.name")}
          placeholder={t("shipping.zone_name_placeholder")}
          value={name}
          onChange={setName}
          onEnter={submit}
        />
        <TextInput
          label={t("common.countries")}
          description={t("shipping.zone_countries_description")}
          placeholder={t("shipping.zone_countries_placeholder")}
          value={countries}
          onChange={setCountries}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={zone ? t("common.save") : t("shipping.zone_dialog_submit_add")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!name.trim() || !countries.trim()}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function ZoneCard({ zone, providers }: { zone: Zone; providers: Provider[] }) {
  const t = useT<"shipping">();
  const [isEditing, setIsEditing] = useState(false);
  const [isAddingOption, setIsAddingOption] = useState(false);
  const [isConfirmingDelete, setIsConfirmingDelete] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const move = (index: number, by: -1 | 1) => {
    const order = zone.options.map((option) => option.id);
    [order[index], order[index + by]] = [order[index + by], order[index]];
    startTransition(async () => setError((await reorderOptions(zone.id, order)).error));
  };

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="large" as="h3">
              {zone.name}
            </Text>
            <HStack gap={1} wrap="wrap">
              {zone.countries.map((country) => (
                <Badge key={country} label={country} />
              ))}
            </HStack>
          </VStack>
          <HStack gap={1}>
            {isConfirmingDelete ? (
              <>
                <Button
                  label={t("common.cancel")}
                  size="sm"
                  variant="ghost"
                  onClick={() => setIsConfirmingDelete(false)}
                />
                <Button
                  label={t("shipping.zone_delete")}
                  size="sm"
                  variant="destructive"
                  isLoading={isPending}
                  onClick={() =>
                    startTransition(async () => {
                      const result = await deleteZone(zone.id);
                      setError(result.error);
                      setIsConfirmingDelete(false);
                    })
                  }
                />
              </>
            ) : (
              <>
                <Button
                  label={t("shipping.option_add")}
                  size="sm"
                  variant="secondary"
                  onClick={() => setIsAddingOption(true)}
                />
                <Button
                  label={t("common.edit")}
                  size="sm"
                  variant="ghost"
                  onClick={() => setIsEditing(true)}
                />
                <Button
                  label={t("common.delete")}
                  size="sm"
                  variant="ghost"
                  onClick={() => setIsConfirmingDelete(true)}
                />
              </>
            )}
          </HStack>
        </HStack>
        {error && <Banner status="error" title={error} />}
        {zone.options.length === 0 ? (
          <Text color="secondary">{t("shipping.zone_no_options")}</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("shipping.options_column_option")}</TableHeaderCell>
                <TableHeaderCell>{t("shipping.options_column_rate")}</TableHeaderCell>
                <TableHeaderCell>{t("shipping.options_column_delivery")}</TableHeaderCell>
                <TableHeaderCell>{t("common.on")}</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {zone.options.map((option, index) => (
                <OptionRow
                  key={option.id}
                  zone={zone}
                  option={option}
                  providers={providers}
                  canMoveUp={index > 0}
                  canMoveDown={index < zone.options.length - 1}
                  onMove={(by) => move(index, by)}
                  onError={setError}
                />
              ))}
            </TableBody>
          </Table>
        )}
      </VStack>
      {isEditing && <ZoneDialog zone={zone} onClose={() => setIsEditing(false)} />}
      {isAddingOption && (
        <OptionDialog zone={zone} providers={providers} onClose={() => setIsAddingOption(false)} />
      )}
    </Card>
  );
}

function OptionRow({
  zone,
  option,
  providers,
  canMoveUp,
  canMoveDown,
  onMove,
  onError,
}: {
  zone: Zone;
  option: Option;
  providers: Provider[];
  canMoveUp: boolean;
  canMoveDown: boolean;
  onMove: (by: -1 | 1) => void;
  onError: (error?: string) => void;
}) {
  const t = useT<"shipping">();
  const format = useFormat();
  const currencies = useCurrencies();
  const [isEditing, setIsEditing] = useState(false);
  const [isConfirmingDelete, setIsConfirmingDelete] = useState(false);
  const [isPending, startTransition] = useTransition();

  return (
    <TableRow>
      <TableCell>
        <VStack gap={0}>
          <HStack gap={1} vAlign="center">
            <Text>{option.name}</Text>
            {option.is_backup && <Badge label={t("shipping.option_backup")} variant="warning" />}
          </HStack>
          <Text type="supporting" color="secondary">
            {option.code}
          </Text>
        </VStack>
      </TableCell>
      <TableCell>
        <Text>{describeRate(t, format, option.rate, currencies)}</Text>
      </TableCell>
      <TableCell>
        <Text color="secondary">
          {option.estimate
            ? t("shipping.option_delivery_days", {
                min: option.estimate.min_days,
                max: option.estimate.max_days,
              })
            : t("common.empty_value")}
        </Text>
      </TableCell>
      <TableCell>
        <Switch
          label={t("shipping.option_offer", { name: option.name })}
          isLabelHidden
          size="sm"
          value={option.enabled}
          changeAction={async (enabled) =>
            onError((await updateOption(zone.id, option.id, { enabled })).error)
          }
        />
      </TableCell>
      <TableCell>
        <HStack gap={1} hAlign="end" vAlign="center">
          {isConfirmingDelete ? (
            <>
              <Button
                label={t("common.cancel")}
                size="sm"
                variant="ghost"
                onClick={() => setIsConfirmingDelete(false)}
              />
              <Button
                label={t("shipping.option_delete_confirm")}
                size="sm"
                variant="destructive"
                isLoading={isPending}
                onClick={() =>
                  startTransition(async () => {
                    onError((await removeOption(zone.id, option.id)).error);
                    setIsConfirmingDelete(false);
                  })
                }
              />
            </>
          ) : (
            <>
              <IconButton
                label={t("shipping.option_move_up", { name: option.name })}
                icon={<ArrowUpIcon />}
                size="sm"
                variant="ghost"
                isDisabled={!canMoveUp}
                onClick={() => onMove(-1)}
              />
              <IconButton
                label={t("shipping.option_move_down", { name: option.name })}
                icon={<ArrowDownIcon />}
                size="sm"
                variant="ghost"
                isDisabled={!canMoveDown}
                onClick={() => onMove(1)}
              />
              <IconButton
                label={t("shipping.option_edit", { name: option.name })}
                icon={<PencilIcon />}
                size="sm"
                variant="ghost"
                onClick={() => setIsEditing(true)}
              />
              <IconButton
                label={t("shipping.option_delete", { name: option.name })}
                icon={<TrashIcon />}
                size="sm"
                variant="ghost"
                onClick={() => setIsConfirmingDelete(true)}
              />
            </>
          )}
        </HStack>
        {isEditing && (
          <OptionDialog
            zone={zone}
            option={option}
            providers={providers}
            onClose={() => setIsEditing(false)}
          />
        )}
      </TableCell>
    </TableRow>
  );
}

/** "Next day" → "next-day": a code from the name until someone types one. */
function codeFrom(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

/** A new option for `zone`, or `option` edited. */
function OptionDialog({
  zone,
  option,
  providers,
  onClose,
}: {
  zone: Zone;
  option?: Option;
  providers: Provider[];
  onClose: () => void;
}) {
  const t = useT<"shipping">();
  const currencies = useCurrencies();
  const [name, setName] = useState(option?.name ?? "");
  const [code, setCode] = useState(option?.code);
  const [draft, setDraft] = useState<RateDraft>(
    option ? toDraft(option.rate, currencies) : emptyDraft("flat", "EUR"),
  );
  const [minDays, setMinDays] = useState(option?.estimate ? String(option.estimate.min_days) : "");
  const [maxDays, setMaxDays] = useState(option?.estimate ? String(option.estimate.max_days) : "");
  const [isBackup, setIsBackup] = useState(option?.is_backup ?? false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const shownCode = code ?? codeFrom(name);

  const submit = () => {
    const built = fromDraft(t, draft, currencies);
    if ("error" in built) return setError(built.error);
    let estimate = null;
    if (minDays.trim() || maxDays.trim()) {
      const min = Number(minDays.trim() || maxDays.trim());
      const max = Number(maxDays.trim() || minDays.trim());
      if (!Number.isInteger(min) || !Number.isInteger(max) || min < 0 || max < 0) {
        return setError(t("shipping.option_delivery_invalid"));
      }
      estimate = { min_days: min, max_days: max };
    }
    const input = { name, code: shownCode, rate: built.rate, estimate, is_backup: isBackup };
    startTransition(async () => {
      const result = option
        ? await updateOption(zone.id, option.id, input)
        : await addOption(zone.id, input);
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={600} purpose="form">
      <DialogHeader
        title={
          option
            ? t("shipping.option_dialog_title_edit", { name: option.name })
            : t("shipping.option_add_to", { zone: zone.name })
        }
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The fields scroll (tier lists grow); the buttons stay in view below them. A flex
          item keeps its content's height unless allowed to shrink (`minHeight: 0`). */}
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          <HStack gap={3}>
            <TextInput
              label={t("common.name")}
              description={t("shipping.option_name_description")}
              placeholder={t("shipping.option_name_placeholder")}
              value={name}
              onChange={setName}
            />
            <TextInput
              label={t("common.code")}
              description={t("shipping.option_code_description")}
              value={shownCode}
              onChange={setCode}
            />
          </HStack>
          <Selector
            label={t("common.price")}
            options={RATE_KINDS.map((kind) => ({
              value: kind,
              label: t(`shipping.rate_kind_${kind}`),
              description: t(`shipping.rate_kind_${kind}_description`),
              disabled: kind === "provider" && providers.length === 0,
            }))}
            value={draft.kind}
            onChange={(value) => {
              const kind = RATE_KINDS.find((k) => k === value);
              if (kind) {
                setDraft(emptyDraft(kind, "prices" in draft ? draft.prices[0].currency : "EUR"));
              }
            }}
          />
          <RateEditor draft={draft} providers={providers} onChange={setDraft} />
          <HStack gap={3}>
            <TextInput
              label={t("shipping.option_days_from")}
              placeholder="2"
              value={minDays}
              onChange={setMinDays}
              width={180}
            />
            <TextInput
              label={t("shipping.option_days_to")}
              placeholder="4"
              value={maxDays}
              onChange={setMaxDays}
              width={180}
            />
          </HStack>
          <CheckboxInput
            label={t("shipping.option_backup_label")}
            description={t("shipping.option_backup_description")}
            value={isBackup}
            onChange={setIsBackup}
          />
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={option ? t("common.save") : t("shipping.option_dialog_submit_add")}
            variant="primary"
            isLoading={isPending}
            isDisabled={!name.trim() || !shownCode}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** The fields of one rate preset, a row (or a tier list) per currency. */
function RateEditor({
  draft,
  providers,
  onChange,
}: {
  draft: RateDraft;
  providers: Provider[];
  onChange: (draft: RateDraft) => void;
}) {
  const t = useT<"shipping">();
  const currencies = useCurrencies();
  if (draft.kind === "provider") {
    return (
      <HStack gap={3}>
        <Selector
          label={t("shipping.rate_app_label")}
          placeholder={t("shipping.rate_app_placeholder")}
          options={providers.map((p) => ({ value: p.key, label: p.name, description: p.key }))}
          value={draft.provider || undefined}
          onChange={(provider) => onChange({ ...draft, provider })}
        />
        <TextInput
          label={t("shipping.rate_service_label")}
          description={t("shipping.rate_service_description")}
          placeholder={t("shipping.rate_service_placeholder")}
          value={draft.service}
          onChange={(service) => onChange({ ...draft, service })}
        />
      </HStack>
    );
  }

  // Each currency's table; the currencies not used yet can be added.
  const used =
    draft.kind === "flat"
      ? draft.prices.map((p) => p.currency)
      : draft.tables.map((table) => table.currency);
  const unused = currencyCodes(currencies).filter((c) => !used.includes(c));
  const addCurrency = () => {
    const currency = unused[0];
    const extra = emptyDraft(draft.kind, currency);
    if (draft.kind === "flat" && extra.kind === "flat") {
      onChange({ ...draft, prices: [...draft.prices, ...extra.prices] });
    } else if (draft.kind === "free_over" && extra.kind === "free_over") {
      onChange({ ...draft, tables: [...draft.tables, ...extra.tables] });
    } else if (
      (draft.kind === "subtotal" || draft.kind === "weight") &&
      (extra.kind === "subtotal" || extra.kind === "weight")
    ) {
      onChange({ ...draft, tables: [...draft.tables, ...extra.tables] });
    }
  };
  const currencyPicker = (currency: Currency, set: (currency: Currency) => void) => (
    <Selector
      label={t("common.currency")}
      isLabelHidden
      size="sm"
      options={currencyCodes(currencies)
        .filter((c) => c === currency || !used.includes(c))
        .map((c) => ({ value: c, label: c }))}
      hasSearch
      value={currency}
      onChange={(value) => set(value)}
    />
  );

  return (
    <VStack gap={3}>
      {draft.kind === "flat" &&
        draft.prices.map((row, i) => {
          const set = (patch: Partial<typeof row>) =>
            onChange({
              ...draft,
              prices: draft.prices.map((r, j) => (j === i ? { ...r, ...patch } : r)),
            });
          return (
            <HStack key={row.currency} gap={2} vAlign="end">
              {currencyPicker(row.currency, (currency) => set({ currency }))}
              <TextInput
                label={t("shipping.rate_price_in", { currency: row.currency })}
                placeholder="4.90"
                value={row.price}
                onChange={(price) => set({ price })}
              />
            </HStack>
          );
        })}
      {draft.kind === "free_over" &&
        draft.tables.map((row, i) => {
          const set = (patch: Partial<typeof row>) =>
            onChange({
              ...draft,
              tables: draft.tables.map((r, j) => (j === i ? { ...r, ...patch } : r)),
            });
          return (
            <HStack key={row.currency} gap={2} vAlign="end">
              {currencyPicker(row.currency, (currency) => set({ currency }))}
              <TextInput
                label={t("shipping.rate_price_in", { currency: row.currency })}
                placeholder="4.90"
                value={row.price}
                onChange={(price) => set({ price })}
              />
              <TextInput
                label={t("shipping.rate_free_from_in", { currency: row.currency })}
                placeholder="50"
                value={row.freeFrom}
                onChange={(freeFrom) => set({ freeFrom })}
              />
            </HStack>
          );
        })}
      {(draft.kind === "subtotal" || draft.kind === "weight") &&
        draft.tables.map((table, i) => {
          const setTable = (patch: Partial<typeof table>) =>
            onChange({
              ...draft,
              tables: draft.tables.map((other, j) => (j === i ? { ...other, ...patch } : other)),
            });
          return (
            <VStack key={table.currency} gap={2}>
              {currencyPicker(table.currency, (currency) => setTable({ currency }))}
              {table.tiers.map((tier, k) => {
                const isLast = k === table.tiers.length - 1;
                const setTier = (patch: Partial<typeof tier>) =>
                  setTable({
                    tiers: table.tiers.map((other, n) =>
                      n === k ? { ...other, ...patch } : other,
                    ),
                  });
                return (
                  // Tiers have no ID of their own; their place is what they are, and their
                  // fields are controlled, so a removed tier can't leave its text behind.
                  // oxlint-disable-next-line react/no-array-index-key
                  <HStack key={k} gap={2} vAlign="end">
                    {isLast ? (
                      <Text color="secondary">
                        {k === 0 ? t("shipping.rate_tier_any") : t("shipping.rate_tier_above")}
                      </Text>
                    ) : (
                      <TextInput
                        label={
                          draft.kind === "weight"
                            ? t("shipping.rate_below_weight")
                            : t("shipping.rate_below_amount", { currency: table.currency })
                        }
                        placeholder={draft.kind === "weight" ? "1000" : "50"}
                        value={tier.upTo}
                        onChange={(upTo) => setTier({ upTo })}
                        width={140}
                      />
                    )}
                    <TextInput
                      label={t("shipping.rate_price_in", { currency: table.currency })}
                      placeholder="4.90"
                      value={tier.price}
                      onChange={(price) => setTier({ price })}
                      width={140}
                    />
                    {table.tiers.length > 1 && !isLast && (
                      <Button
                        label={t("common.remove")}
                        size="sm"
                        variant="ghost"
                        onClick={() => setTable({ tiers: table.tiers.filter((_, n) => n !== k) })}
                      />
                    )}
                  </HStack>
                );
              })}
              <HStack>
                <Button
                  label={t("shipping.rate_tier_add")}
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    setTable({
                      tiers: [
                        ...table.tiers.slice(0, -1),
                        { upTo: "", price: "" },
                        table.tiers[table.tiers.length - 1],
                      ],
                    })
                  }
                />
              </HStack>
            </VStack>
          );
        })}
      {unused.length > 0 && (
        <HStack>
          <Button
            label={t("shipping.rate_currency_add")}
            size="sm"
            variant="ghost"
            onClick={addCurrency}
          />
        </HStack>
      )}
    </VStack>
  );
}
