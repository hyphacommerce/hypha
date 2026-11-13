"use client";

// Who and which carts the discount is for. Every condition set must hold; a list matches
// any of its values (any of these markets, any of these groups).

import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState } from "react";

import { type ConditionsDraft, amountMarkets } from "@/lib/discounts/draft";
import { useT } from "@/lib/use-t";

import {
  AmountsByMarket,
  type DiscountOptions,
  IdTokens,
  type Option,
  type SectionProps,
  FieldError,
  fieldStatus,
} from "./fields";
import { SearchPicker } from "./search-picker";

/** Options plus any ID the draft names that isn't among them (not readable), by ID. */
function withUnknown(options: readonly Option[], ids: readonly string[]): Option[] {
  return [
    ...options,
    ...ids
      .filter((id) => !options.some((o) => o.value === id))
      .map((id) => ({ value: id, label: id })),
  ];
}

export function ConditionsSection({
  draft,
  errors,
  onChange,
  isReadOnly,
  options,
  names,
  onNames,
}: SectionProps & {
  options: DiscountOptions;
  names: Readonly<Record<string, string>>;
  /** Names of customers picked here, for the tokens and the summary. */
  onNames: (names: Record<string, string>) => void;
}) {
  const t = useT<"discounts">();
  const [isPicking, setIsPicking] = useState(false);
  const c = draft.conditions;
  const set = (change: Partial<ConditionsDraft>, field: string) =>
    onChange({ ...draft, conditions: { ...c, ...change } }, [`conditions.${field}`]);
  const markets = options.markets.map((m) => ({ value: m.id, label: m.name }));
  const groups = withUnknown(options.groups, c.customerGroups);

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("discounts.conditions_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("discounts.conditions_hint")}
          </Text>
        </VStack>

        <MultiSelector
          label={t("common.markets")}
          description={t("discounts.conditions_markets_hint")}
          placeholder={t("discounts.conditions_markets_placeholder")}
          options={withUnknown(markets, c.markets)}
          value={c.markets}
          onChange={(chosen) => set({ markets: chosen }, "markets")}
          hasSearch
          isDisabled={isReadOnly}
          status={fieldStatus(errors["conditions.markets"])}
          width="100%"
        />
        <MultiSelector
          label={t("discounts.conditions_channels")}
          placeholder={t("discounts.conditions_channels_placeholder")}
          options={withUnknown(options.channels, c.channels)}
          value={c.channels}
          onChange={(chosen) => set({ channels: chosen }, "channels")}
          isDisabled={isReadOnly}
          status={fieldStatus(errors["conditions.channels"])}
          width="100%"
        />
        {groups.length > 0 && (
          <MultiSelector
            label={t("discounts.conditions_groups")}
            description={t("discounts.conditions_groups_hint")}
            placeholder={t("discounts.conditions_groups_placeholder")}
            options={groups}
            value={c.customerGroups}
            onChange={(chosen) => set({ customerGroups: chosen }, "customerGroups")}
            hasSearch
            isDisabled={isReadOnly}
            status={fieldStatus(errors["conditions.customerGroups"])}
            width="100%"
          />
        )}
        <VStack gap={2} hAlign="start">
          <Text weight="medium">{t("common.customers")}</Text>
          {c.customers.length === 0 && (
            <Text type="supporting" color="secondary">
              {t("discounts.conditions_customers_anyone")}
            </Text>
          )}
          <FieldError message={errors["conditions.customers"]} />
          <IdTokens
            ids={c.customers}
            names={names}
            onRemove={
              isReadOnly
                ? undefined
                : (id) => set({ customers: c.customers.filter((x) => x !== id) }, "customers")
            }
          />
          {!isReadOnly && (
            <Button
              label={t("discounts.conditions_choose_customers")}
              variant="secondary"
              onClick={() => setIsPicking(true)}
            />
          )}
          {isPicking && (
            <SearchPicker
              source="customers"
              title={t("discounts.conditions_customers_picker")}
              chosen={c.customers}
              onConfirm={(picked) => {
                onNames(Object.fromEntries(picked.map((p) => [p.id, p.label])));
                set({ customers: [...c.customers, ...picked.map((p) => p.id)] }, "customers");
                setIsPicking(false);
              }}
              onClose={() => setIsPicking(false)}
            />
          )}
        </VStack>
        <Switch
          label={t("discounts.conditions_signed_in")}
          value={c.signedIn}
          onChange={(signedIn) => set({ signedIn }, "signedIn")}
          isDisabled={isReadOnly}
        />

        <Divider />
        <Switch
          label={t("discounts.conditions_min_subtotal")}
          description={t("discounts.conditions_min_subtotal_hint")}
          value={c.minSubtotal.enabled}
          onChange={(enabled) => set({ minSubtotal: { ...c.minSubtotal, enabled } }, "minSubtotal")}
          isDisabled={isReadOnly}
        />
        {c.minSubtotal.enabled && (
          <AmountsByMarket
            label={t("discounts.conditions_at_least")}
            placeholder={t("discounts.conditions_no_minimum")}
            amounts={c.minSubtotal.amounts}
            markets={amountMarkets(c, options.markets)}
            path="conditions.minSubtotal.amounts"
            errors={errors}
            onChange={(amounts) =>
              set({ minSubtotal: { ...c.minSubtotal, amounts } }, "minSubtotal.amounts")
            }
            isReadOnly={isReadOnly}
          />
        )}
        <Switch
          label={t("discounts.conditions_min_quantity")}
          description={t("discounts.conditions_min_quantity_hint")}
          value={c.minQuantity.enabled}
          onChange={(enabled) => set({ minQuantity: { ...c.minQuantity, enabled } }, "minQuantity")}
          isDisabled={isReadOnly}
        />
        {c.minQuantity.enabled && (
          <TextInput
            label={t("discounts.conditions_at_least_quantity")}
            value={c.minQuantity.quantity}
            onChange={(quantity) =>
              set({ minQuantity: { ...c.minQuantity, quantity } }, "minQuantity.quantity")
            }
            isDisabled={isReadOnly}
            status={fieldStatus(errors["conditions.minQuantity.quantity"])}
            width={200}
          />
        )}
      </VStack>
    </Card>
  );
}
