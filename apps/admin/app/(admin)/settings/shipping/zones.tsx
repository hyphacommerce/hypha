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
import type { AdminSchemas } from "@hypha/sdk/admin";
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
  const [isAdding, setIsAdding] = useState(false);

  return (
    <VStack gap={4}>
      <HStack gap={2} hAlign="between" vAlign="center">
        <Text type="display-3" as="h2">
          Zones
        </Text>
        <Button label="Add zone" variant="secondary" onClick={() => setIsAdding(true)} />
      </HStack>
      {zones.length === 0 && (
        <Card>
          <EmptyState
            title="No shipping zones yet"
            description="Carts to a country outside every zone can't choose shipping, so physical products can't be checked out there."
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
        title={zone ? "Edit zone" : "Add zone"}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextInput
          label="Name"
          placeholder="Europe"
          value={name}
          onChange={setName}
          onEnter={submit}
        />
        <TextInput
          label="Countries"
          description="Two-letter codes, separated by commas. A country is in one zone at most."
          placeholder="HR, SI, DE"
          value={countries}
          onChange={setCountries}
          onEnter={submit}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={zone ? "Save" : "Add zone"}
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
                  label="Cancel"
                  size="sm"
                  variant="ghost"
                  onClick={() => setIsConfirmingDelete(false)}
                />
                <Button
                  label="Delete zone"
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
                  label="Add option"
                  size="sm"
                  variant="secondary"
                  onClick={() => setIsAddingOption(true)}
                />
                <Button label="Edit" size="sm" variant="ghost" onClick={() => setIsEditing(true)} />
                <Button
                  label="Delete"
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
          <Text color="secondary">No options yet: carts here can't choose shipping.</Text>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Option</TableHeaderCell>
                <TableHeaderCell>Rate</TableHeaderCell>
                <TableHeaderCell>Delivery</TableHeaderCell>
                <TableHeaderCell>On</TableHeaderCell>
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
            {option.is_backup && <Badge label="Backup" variant="warning" />}
          </HStack>
          <Text type="supporting" color="secondary">
            {option.code}
          </Text>
        </VStack>
      </TableCell>
      <TableCell>
        <Text>{describeRate(option.rate, currencies)}</Text>
      </TableCell>
      <TableCell>
        <Text color="secondary">
          {option.estimate ? `${option.estimate.min_days}–${option.estimate.max_days} days` : "—"}
        </Text>
      </TableCell>
      <TableCell>
        <Switch
          label={`Offer ${option.name}`}
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
                label="Cancel"
                size="sm"
                variant="ghost"
                onClick={() => setIsConfirmingDelete(false)}
              />
              <Button
                label="Delete option"
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
                label={`Move ${option.name} up`}
                icon={<ArrowUpIcon />}
                size="sm"
                variant="ghost"
                isDisabled={!canMoveUp}
                onClick={() => onMove(-1)}
              />
              <IconButton
                label={`Move ${option.name} down`}
                icon={<ArrowDownIcon />}
                size="sm"
                variant="ghost"
                isDisabled={!canMoveDown}
                onClick={() => onMove(1)}
              />
              <IconButton
                label={`Edit ${option.name}`}
                icon={<PencilIcon />}
                size="sm"
                variant="ghost"
                onClick={() => setIsEditing(true)}
              />
              <IconButton
                label={`Delete ${option.name}`}
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
    const built = fromDraft(draft, currencies);
    if ("error" in built) return setError(built.error);
    let estimate = null;
    if (minDays.trim() || maxDays.trim()) {
      const min = Number(minDays.trim() || maxDays.trim());
      const max = Number(maxDays.trim() || minDays.trim());
      if (!Number.isInteger(min) || !Number.isInteger(max) || min < 0 || max < 0) {
        return setError("Enter delivery days as whole numbers, like 2 and 4.");
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
        title={option ? `Edit ${option.name}` : `Add an option to ${zone.name}`}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      {/* The fields scroll (tier lists grow); the buttons stay in view below them. A flex
          item keeps its content's height unless allowed to shrink (`minHeight: 0`). */}
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={4} padding={4}>
          <HStack gap={3}>
            <TextInput
              label="Name"
              description="What shoppers see."
              placeholder="Standard"
              value={name}
              onChange={setName}
            />
            <TextInput
              label="Code"
              description="Stable, for apps and reports."
              value={shownCode}
              onChange={setCode}
            />
          </HStack>
          <Selector
            label="Price"
            options={RATE_KINDS.map((kind) => ({
              value: kind.value,
              label: kind.label,
              description: kind.description,
              disabled: kind.value === "provider" && providers.length === 0,
            }))}
            value={draft.kind}
            onChange={(value) => {
              const kind = RATE_KINDS.find((k) => k.value === value)?.value;
              if (kind) {
                setDraft(emptyDraft(kind, "prices" in draft ? draft.prices[0].currency : "EUR"));
              }
            }}
          />
          <RateEditor draft={draft} providers={providers} onChange={setDraft} />
          <HStack gap={3}>
            <TextInput
              label="Delivery, days from"
              placeholder="2"
              value={minDays}
              onChange={setMinDays}
              width={180}
            />
            <TextInput
              label="Days to"
              placeholder="4"
              value={maxDays}
              onChange={setMaxDays}
              width={180}
            />
          </HStack>
          <CheckboxInput
            label="Backup"
            description="Offered only when every rate app option of this zone fails, so an outage doesn't stop sales."
            value={isBackup}
            onChange={setIsBackup}
          />
        </VStack>
      </div>
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={option ? "Save" : "Add option"}
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
  const currencies = useCurrencies();
  if (draft.kind === "provider") {
    return (
      <HStack gap={3}>
        <Selector
          label="Rate app"
          placeholder="Choose an app"
          options={providers.map((p) => ({ value: p.key, label: p.name, description: p.key }))}
          value={draft.provider || undefined}
          onChange={(provider) => onChange({ ...draft, provider })}
        />
        <TextInput
          label="Service"
          description="The code the app quotes it under."
          placeholder="express"
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
      : draft.tables.map((t) => t.currency);
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
      label="Currency"
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
                label={`Price (${row.currency})`}
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
                label={`Price (${row.currency})`}
                placeholder="4.90"
                value={row.price}
                onChange={(price) => set({ price })}
              />
              <TextInput
                label={`Free from (${row.currency})`}
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
              tables: draft.tables.map((t, j) => (j === i ? { ...t, ...patch } : t)),
            });
          const unit = draft.kind === "weight" ? "g" : table.currency;
          return (
            <VStack key={table.currency} gap={2}>
              {currencyPicker(table.currency, (currency) => setTable({ currency }))}
              {table.tiers.map((tier, k) => {
                const isLast = k === table.tiers.length - 1;
                const setTier = (patch: Partial<typeof tier>) =>
                  setTable({
                    tiers: table.tiers.map((t, n) => (n === k ? { ...t, ...patch } : t)),
                  });
                return (
                  // Tiers have no ID of their own; their place is what they are, and their
                  // fields are controlled, so a removed tier can't leave its text behind.
                  // oxlint-disable-next-line react/no-array-index-key
                  <HStack key={k} gap={2} vAlign="end">
                    {isLast ? (
                      <Text color="secondary">{k === 0 ? "Any" : "Above that"}</Text>
                    ) : (
                      <TextInput
                        label={`Below (${unit})`}
                        placeholder={draft.kind === "weight" ? "1000" : "50"}
                        value={tier.upTo}
                        onChange={(upTo) => setTier({ upTo })}
                        width={140}
                      />
                    )}
                    <TextInput
                      label={`Price (${table.currency})`}
                      placeholder="4.90"
                      value={tier.price}
                      onChange={(price) => setTier({ price })}
                      width={140}
                    />
                    {table.tiers.length > 1 && !isLast && (
                      <Button
                        label="Remove"
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
                  label="Add tier"
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
          <Button label="Add a currency" size="sm" variant="ghost" onClick={addCurrency} />
        </HStack>
      )}
    </VStack>
  );
}
