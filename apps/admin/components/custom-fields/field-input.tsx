"use client";

// One custom field's input, by its type (design note 0033, §3 and §9): what staff type,
// held as a `FieldInput` until `valueOf` turns it into the field's JSON. Shared by every
// card that shows custom fields, and later by app action inputs.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { DateInput } from "@astryxdesign/core/DateInput";
import { DateTimeInput, type ISODateTimeString } from "@astryxdesign/core/DateTimeInput";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Tokenizer } from "@astryxdesign/core/Tokenizer";
import type { SearchableItem, SearchSource } from "@astryxdesign/core/Typeahead";
import type { ISODateString } from "@astryxdesign/core/utils";
import type { ReactNode } from "react";

import { useCurrencies } from "@/components/currencies";
import { RichTextField } from "@/components/rich-text-field";
import {
  type Definition,
  type FieldInput,
  managerLabel,
  parseType,
  unitsOf,
} from "@/lib/custom-fields";
import { currencyCodes } from "@/lib/format";

import { ReferenceInput, ReferenceListPicker } from "./reference-input";

/** Lists are typed in: the source offers nothing, `hasCreate` adds. */
const noSuggestions: SearchSource = { search: () => [], bootstrap: () => [] };

const asItems = (items: string[]): SearchableItem[] =>
  items.map((item, i) => ({ id: `${i}:${item}`, label: item }));

const isDay = (text: string): text is ISODateString => /^\d{4}-\d{2}-\d{2}$/.test(text);

/** `YYYY-MM-DDTHH:mm`, as the date-time input takes it. */
const isMinute = (text: string): text is ISODateTimeString =>
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?$/.test(text);

/** What a type's text looks like, as a placeholder. */
const PLACEHOLDERS: Record<string, string> = {
  integer: "42",
  decimal: "12.50",
  url: "https://…",
  date: "2026-10-20",
  date_time: "2026-10-20T09:30",
  money: "12.50 EUR",
  reference: "prod_…",
};

/** The field's description, with what it is and who manages it. */
function descriptionOf(definition: Definition, extra?: string): string | undefined {
  const parts = [definition.description, extra].filter(Boolean);
  return parts.length > 0 ? parts.join(" ") : undefined;
}

export function CustomFieldInput({
  definition,
  input,
  onChange,
  isDisabled,
  error,
}: {
  definition: Definition;
  input: FieldInput;
  onChange: (input: FieldInput) => void;
  isDisabled?: boolean;
  /** What's wrong with the value, from the form or the API. */
  error?: string;
}) {
  const currencies = useCurrencies();
  const { base, isList } = parseType(definition.type);
  const label = definition.name;
  const status = error ? { type: "error" as const, message: error } : undefined;
  const v = definition.validations;
  const common = { label, isOptional: true, isDisabled, status };

  let control: ReactNode;
  if (input.kind === "list") {
    if (base === "choice") {
      control = (
        <MultiSelector
          {...common}
          description={descriptionOf(definition)}
          options={(v.choices ?? []).map((c) => ({ value: c.key, label: c.label }))}
          value={input.items}
          onChange={(items) => onChange({ kind: "list", items })}
          hasSearch={(v.choices?.length ?? 0) > 8}
          triggerDisplay="labels"
          width="100%"
        />
      );
    } else {
      const unit =
        base === "measurement"
          ? `Each with its unit: ${unitsOf(v.dimension).join(", ")}.`
          : undefined;
      const money = base === "money" ? "Each amount with its currency, like 12.50 EUR." : undefined;
      control = (
        <VStack gap={1}>
          <Tokenizer
            {...common}
            description={descriptionOf(definition, unit ?? money)}
            placeholder={PLACEHOLDERS[base] ? `Add, like ${PLACEHOLDERS[base]}` : "Add…"}
            searchSource={noSuggestions}
            hasCreate
            debounceMs={0}
            maxEntries={v.max_items ?? 128}
            value={asItems(input.items)}
            onChange={(items) => onChange({ kind: "list", items: items.map((i) => i.label) })}
          />
          {base === "reference" && !isDisabled && (
            <ReferenceListPicker
              kind={v.kind ?? "product"}
              onPick={(ids) =>
                onChange({
                  kind: "list",
                  items: [...input.items, ...ids.filter((id) => !input.items.includes(id))],
                })
              }
            />
          )}
        </VStack>
      );
    }
  } else if (input.kind === "boolean") {
    control = (
      <Switch
        label={label}
        description={descriptionOf(definition)}
        value={input.value === true}
        isDisabled={isDisabled}
        status={status}
        onChange={(value) => onChange({ kind: "boolean", value })}
      />
    );
  } else if (input.kind === "money") {
    const allowed = v.currencies?.length ? v.currencies : currencyCodes(currencies);
    control = (
      <HStack gap={2} vAlign="end">
        <TextInput
          {...common}
          description={descriptionOf(definition)}
          placeholder="0.00"
          value={input.amount}
          onChange={(amount) => onChange({ ...input, amount })}
        />
        <Selector
          label={`Currency of ${label}`}
          isLabelHidden
          options={allowed.map((c) => ({ value: c, label: c }))}
          value={input.currency}
          isDisabled={isDisabled || allowed.length === 1}
          hasSearch={allowed.length > 8}
          onChange={(currency: string) => onChange({ ...input, currency })}
          width={110}
        />
      </HStack>
    );
  } else if (input.kind === "measurement") {
    const units = unitsOf(v.dimension);
    control = (
      <HStack gap={2} vAlign="end">
        <TextInput
          {...common}
          description={descriptionOf(definition)}
          placeholder="1.5"
          value={input.value}
          onChange={(value) => onChange({ ...input, value })}
        />
        <Selector
          label={`Unit of ${label}`}
          isLabelHidden
          options={units.map((u) => ({ value: u, label: u.replace("_", " ") }))}
          value={input.unit}
          isDisabled={isDisabled}
          onChange={(unit: string) => onChange({ ...input, unit })}
          width={100}
        />
      </HStack>
    );
  } else {
    const text = input.text;
    const set = (next: string) => onChange({ kind: "text", text: next });
    switch (base) {
      case "multiline_text":
        control = (
          <TextArea
            {...common}
            description={descriptionOf(definition)}
            value={text}
            onChange={set}
            rows={4}
            maxLength={v.max_length ?? undefined}
          />
        );
        break;
      case "rich_text":
        control = isDisabled ? (
          <TextArea {...common} description={descriptionOf(definition)} value={text} rows={4} />
        ) : (
          <VStack gap={1}>
            <RichTextField
              label={label}
              description={descriptionOf(definition)}
              isOptional
              value={text}
              onChange={set}
              minHeight={120}
            />
            {error && <Banner status="error" title={error} />}
          </VStack>
        );
        break;
      case "json":
        control = (
          <TextArea
            {...common}
            description={descriptionOf(definition, "JSON, checked when saving.")}
            value={text}
            onChange={set}
            rows={5}
            hasSpellCheck={false}
          />
        );
        break;
      case "choice":
        control = (
          <Selector
            {...common}
            description={descriptionOf(definition)}
            placeholder="None"
            options={(v.choices ?? []).map((c) => ({ value: c.key, label: c.label }))}
            value={text || null}
            hasClear
            hasSearch={(v.choices?.length ?? 0) > 8}
            onChange={(value: string | null) => set(value ?? "")}
            width="100%"
          />
        );
        break;
      case "date":
        control = (
          <DateInput
            {...common}
            description={descriptionOf(definition)}
            value={isDay(text) ? text : undefined}
            hasClear
            weekStartsOn="mon"
            onChange={(value) => set(value ?? "")}
          />
        );
        break;
      case "date_time":
        control = (
          <DateTimeInput
            {...common}
            description={descriptionOf(definition, "In UTC.")}
            value={isMinute(text) ? text : undefined}
            hasClear
            hourFormat="24h"
            weekStartsOn="mon"
            onChange={(value) => set(value ?? "")}
          />
        );
        break;
      case "reference":
        control = (
          <ReferenceInput
            label={label}
            description={descriptionOf(definition)}
            kind={v.kind ?? "product"}
            value={text}
            onChange={set}
            isDisabled={isDisabled}
            error={error}
          />
        );
        break;
      default:
        control = (
          <TextInput
            {...common}
            description={descriptionOf(definition)}
            placeholder={PLACEHOLDERS[base]}
            value={text}
            onChange={set}
          />
        );
    }
  }

  const manager = managerLabel(definition);
  const marks = [
    definition.personal && <Badge key="personal" label="Personal" variant="warning" />,
    manager && <Badge key="manager" label={manager} variant="info" />,
    isDisabled && <Badge key="read" label="Read-only" />,
  ].filter(Boolean);

  return (
    <VStack gap={1}>
      {control}
      {(marks.length > 0 || isList) && (
        <HStack gap={1} wrap="wrap" vAlign="center">
          {marks}
          {isList && input.kind === "list" && input.items.length > 0 && (
            <Text type="supporting" color="secondary">
              {input.items.length} {input.items.length === 1 ? "item" : "items"}
            </Text>
          )}
        </HStack>
      )}
    </VStack>
  );
}
