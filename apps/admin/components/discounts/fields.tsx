"use client";

// The pieces the discount form's sections share: what every section takes, amounts typed
// per market, chosen IDs as removable tokens, and a reward's value.

import { FieldStatus } from "@astryxdesign/core/FieldStatus";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Token } from "@astryxdesign/core/Token";

import type { DiscountDraft, PerMarketText, ValueDraft } from "@/lib/discounts/draft";
import type { FieldErrors, MarketOption } from "@/lib/discounts/validate";
import { useT } from "@/lib/use-t";

export type Option = { value: string; label: string };

/** What the form chooses from: the store's markets, channels, groups and categories. */
export type DiscountOptions = {
  markets: readonly MarketOption[];
  channels: readonly Option[];
  /** None without `customers:read`. */
  groups: readonly Option[];
  categories: readonly Option[];
};

/**
 * What every section takes. `onChange` gets the whole next draft and the paths it
 * changed, so the form can clear their errors.
 */
export type SectionProps = {
  draft: DiscountDraft;
  errors: FieldErrors;
  onChange: (draft: DiscountDraft, paths: string[]) => void;
  isReadOnly?: boolean;
};

export const fieldStatus = (message: string | undefined) =>
  message ? { type: "error" as const, message } : undefined;

/** A field's error where the field isn't an input (a list of tokens, a group of inputs). */
export function FieldError({ message }: { message: string | undefined }) {
  return message ? <FieldStatus type="error" message={message} variant="detached" /> : null;
}

/** One amount input per market, labelled with its name and currency. */
export function AmountsByMarket({
  label,
  description,
  amounts,
  markets,
  path,
  errors,
  onChange,
  isReadOnly,
  placeholder,
}: {
  label: string;
  description?: string;
  /** What a blank input means; "Not offered" unless given. */
  placeholder?: string;
  amounts: PerMarketText;
  markets: readonly MarketOption[];
  path: string;
  errors: FieldErrors;
  onChange: (amounts: PerMarketText) => void;
  isReadOnly?: boolean;
}) {
  const t = useT<"discounts">();
  const own = errors[path];
  return (
    <VStack gap={2}>
      <VStack gap={1}>
        <Text weight="medium">{label}</Text>
        {description && (
          <Text type="supporting" color="secondary">
            {description}
          </Text>
        )}
      </VStack>
      {markets.length === 0 ? (
        <Text color="secondary">{t("discounts.amounts_no_market")}</Text>
      ) : (
        <HStack gap={3} wrap="wrap">
          {markets.map((m) => (
            <TextInput
              key={m.id}
              label={`${m.name} (${m.currency})`}
              placeholder={placeholder ?? t("discounts.amounts_not_offered")}
              value={amounts[m.id] ?? ""}
              onChange={(text) => onChange({ ...amounts, [m.id]: text })}
              isDisabled={isReadOnly}
              status={fieldStatus(errors[`${path}.${m.id}`])}
              width={180}
            />
          ))}
        </HStack>
      )}
      <FieldError message={own} />
    </VStack>
  );
}

/** Chosen IDs as tokens, by name where it's known. */
export function IdTokens({
  ids,
  names,
  onRemove,
}: {
  ids: readonly string[];
  names: Readonly<Record<string, string>>;
  onRemove?: (id: string) => void;
}) {
  if (ids.length === 0) return null;
  return (
    <HStack gap={1} wrap="wrap">
      {ids.map((id) => (
        <Token
          key={id}
          size="sm"
          label={names[id] ?? id}
          onRemove={onRemove ? () => onRemove(id) : undefined}
        />
      ))}
    </HStack>
  );
}

/**
 * A reward's value: its type, then the percentage or the amounts, and for an amount off
 * items how it's spread. `free` adds a "Free" choice (shipping), which is 100% off.
 */
export function ValueFields({
  value,
  types,
  path,
  markets,
  errors,
  onChange,
  isReadOnly,
  hasAllocation = false,
  hasFree = false,
}: {
  value: ValueDraft;
  types: readonly ValueDraft["type"][];
  path: string;
  markets: readonly MarketOption[];
  errors: FieldErrors;
  onChange: (value: ValueDraft) => void;
  isReadOnly?: boolean;
  hasAllocation?: boolean;
  hasFree?: boolean;
}) {
  const t = useT<"discounts">();
  const isFree = hasFree && value.type === "percent" && value.percent.trim() === "100";
  const choice = isFree ? "free" : value.type;
  const choose = (next: string) => {
    if (next === "free") onChange({ ...value, type: "percent", percent: "100" });
    else if (next === "percent" || next === "amount" || next === "fixed_price") {
      const percent = next === "percent" && isFree ? "" : value.percent;
      onChange({ ...value, type: next, percent });
    }
  };
  return (
    <VStack gap={3}>
      <SegmentedControl
        label={t("discounts.value_title")}
        value={choice}
        onChange={choose}
        isDisabled={isReadOnly}
      >
        {hasFree && <SegmentedControlItem value="free" label={t("discounts.value_free")} />}
        {types.map((type) => (
          <SegmentedControlItem key={type} value={type} label={t(`discounts.value_type_${type}`)} />
        ))}
      </SegmentedControl>
      {value.type === "percent" && !isFree && (
        <TextInput
          label={t("discounts.value_percent")}
          description={t("discounts.value_percent_hint")}
          value={value.percent}
          onChange={(percent) => onChange({ ...value, percent })}
          isDisabled={isReadOnly}
          status={fieldStatus(errors[`${path}.percent`])}
          width={200}
        />
      )}
      {(value.type === "amount" || value.type === "fixed_price") && (
        <AmountsByMarket
          label={value.type === "amount" ? t("discounts.value_type_amount") : t("common.price")}
          description={
            value.type === "amount"
              ? t("discounts.value_amount_hint")
              : t("discounts.value_fixed_price_hint")
          }
          amounts={value.amounts}
          markets={markets}
          path={`${path}.amounts`}
          errors={errors}
          onChange={(amounts) => onChange({ ...value, amounts })}
          isReadOnly={isReadOnly}
        />
      )}
      {hasAllocation && value.type === "amount" && (
        <RadioList
          label={t("discounts.value_allocation")}
          value={value.allocation}
          onChange={(allocation) =>
            onChange({
              ...value,
              allocation:
                allocation === "each" ? "each" : allocation === "cheapest" ? "cheapest" : "across",
            })
          }
          isDisabled={isReadOnly}
        >
          <RadioListItem
            value="across"
            label={t("discounts.value_allocation_across")}
            description={t("discounts.value_allocation_across_hint")}
          />
          <RadioListItem
            value="each"
            label={t("discounts.value_allocation_each")}
            description={t("discounts.value_allocation_each_hint")}
          />
          <RadioListItem
            value="cheapest"
            label={t("discounts.value_allocation_cheapest")}
            description={t("discounts.value_allocation_cheapest_hint")}
          />
        </RadioList>
      )}
      {hasAllocation && value.type === "amount" && value.allocation === "cheapest" && (
        <TextInput
          label={t("discounts.value_cheapest_units")}
          value={value.cheapestUnits}
          onChange={(cheapestUnits) => onChange({ ...value, cheapestUnits })}
          isDisabled={isReadOnly}
          status={fieldStatus(errors[`${path}.cheapestUnits`])}
          width={200}
        />
      )}
      <FieldError message={errors[`${path}.type`]} />
    </VStack>
  );
}
