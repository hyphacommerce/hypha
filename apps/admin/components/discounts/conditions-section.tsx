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

import { type ConditionsDraft, amountMarkets } from "@/lib/discounts/draft";

import {
  AmountsByMarket,
  type DiscountOptions,
  IdTokens,
  type Option,
  type SectionProps,
  FieldError,
  fieldStatus,
} from "./fields";

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
  onPickCustomers,
}: SectionProps & {
  options: DiscountOptions;
  names: Readonly<Record<string, string>>;
  /** Opens the page's customer picker; without one, customers can only be removed. */
  onPickCustomers?: () => void;
}) {
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
            Conditions
          </Text>
          <Text type="supporting" color="secondary">
            Leave a list empty for no limit. Every condition that&apos;s set must hold; within a
            list, any one value is enough.
          </Text>
        </VStack>

        <MultiSelector
          label="Markets"
          description="Amounts are typed for these markets only."
          placeholder="Every market"
          options={withUnknown(markets, c.markets)}
          value={c.markets}
          onChange={(chosen) => set({ markets: chosen }, "markets")}
          hasSearch
          isDisabled={isReadOnly}
          status={fieldStatus(errors["conditions.markets"])}
          width="100%"
        />
        <MultiSelector
          label="Sales channels"
          placeholder="Every channel"
          options={withUnknown(options.channels, c.channels)}
          value={c.channels}
          onChange={(chosen) => set({ channels: chosen }, "channels")}
          isDisabled={isReadOnly}
          status={fieldStatus(errors["conditions.channels"])}
          width="100%"
        />
        {groups.length > 0 && (
          <MultiSelector
            label="Customer groups"
            description="Only signed-in customers in one of them."
            placeholder="Everyone"
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
          <Text weight="medium">Customers</Text>
          {c.customers.length === 0 && (
            <Text type="supporting" color="secondary">
              Anyone. Name customers to make it only theirs.
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
          {!isReadOnly && onPickCustomers && (
            <Button label="Choose customers" variant="secondary" onClick={onPickCustomers} />
          )}
        </VStack>
        <Switch
          label="Signed-in customers only"
          value={c.signedIn}
          onChange={(signedIn) => set({ signedIn }, "signedIn")}
          isDisabled={isReadOnly}
        />

        <Divider />
        <Switch
          label="Minimum subtotal"
          description="Of the items it applies to, after product discounts."
          value={c.minSubtotal.enabled}
          onChange={(enabled) => set({ minSubtotal: { ...c.minSubtotal, enabled } }, "minSubtotal")}
          isDisabled={isReadOnly}
        />
        {c.minSubtotal.enabled && (
          <AmountsByMarket
            label="At least"
            placeholder="No minimum"
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
          label="Minimum quantity"
          description="Of the items it applies to."
          value={c.minQuantity.enabled}
          onChange={(enabled) => set({ minQuantity: { ...c.minQuantity, enabled } }, "minQuantity")}
          isDisabled={isReadOnly}
        />
        {c.minQuantity.enabled && (
          <TextInput
            label="At least"
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
