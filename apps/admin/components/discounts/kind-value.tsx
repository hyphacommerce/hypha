"use client";

// The Value card for the kinds beyond a standard discount: buy X get Y (what must be
// bought, how many are got and how much off them) and volume tiers (what's counted, and
// each tier's threshold and value).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";

import {
  type BuyXGetYDraft,
  MAX_TIERS,
  type TierDraft,
  type TieredDraft,
  emptyValue,
} from "@/lib/discounts/draft";
import type { MarketOption } from "@/lib/discounts/validate";

import { AmountsByMarket, type SectionProps, ValueFields, fieldStatus } from "./fields";

type KindProps<K> = SectionProps & { kind: K; markets: readonly MarketOption[] };

export function BuyXGetYValue({
  draft,
  kind,
  errors,
  onChange,
  isReadOnly,
  markets,
}: KindProps<BuyXGetYDraft>) {
  const set = (change: Partial<BuyXGetYDraft>, paths: string[]) =>
    onChange({ ...draft, kind: { ...kind, ...change } }, paths);
  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Value
          </Text>
          <Text type="supporting" color="secondary">
            Each time the cart has what&apos;s bought, it gets the cheapest units of what&apos;s got
            for less: free, a percentage off or an amount off each.
          </Text>
        </VStack>
        <SegmentedControl
          label="Customer buys at least"
          value={kind.buy.type}
          onChange={(type) =>
            set({ buy: { ...kind.buy, type: type === "amount" ? "amount" : "quantity" } }, [
              "kind.buy",
            ])
          }
          isDisabled={isReadOnly}
        >
          <SegmentedControlItem value="quantity" label="A number of items" />
          <SegmentedControlItem value="amount" label="An amount spent" />
        </SegmentedControl>
        {kind.buy.type === "quantity" ? (
          <TextInput
            label="Items bought"
            value={kind.buy.quantity}
            onChange={(quantity) => set({ buy: { ...kind.buy, quantity } }, ["kind.buy.quantity"])}
            isDisabled={isReadOnly}
            status={fieldStatus(errors["kind.buy.quantity"])}
            width={200}
          />
        ) : (
          <AmountsByMarket
            label="Spent on what's bought"
            amounts={kind.buy.amounts}
            markets={markets}
            path="kind.buy.amounts"
            errors={errors}
            onChange={(amounts) => set({ buy: { ...kind.buy, amounts } }, ["kind.buy.amounts"])}
            isReadOnly={isReadOnly}
          />
        )}
        <Divider />
        <TextInput
          label="Items they get"
          description="The cheapest units of what's got that the cart holds."
          value={kind.getQuantity}
          onChange={(getQuantity) => set({ getQuantity }, ["kind.getQuantity"])}
          isDisabled={isReadOnly}
          status={fieldStatus(errors["kind.getQuantity"])}
          width={200}
        />
        <ValueFields
          value={kind.value}
          types={["percent", "amount"]}
          path="kind.value"
          markets={markets}
          errors={errors}
          onChange={(value) => set({ value }, ["kind.value"])}
          isReadOnly={isReadOnly}
          hasFree
        />
        <TextInput
          label="Uses per order"
          isOptional
          description="How many times one order gets it. Blank: as often as the cart qualifies."
          value={kind.usesPerOrder}
          onChange={(usesPerOrder) => set({ usesPerOrder }, ["kind.usesPerOrder"])}
          isDisabled={isReadOnly}
          status={fieldStatus(errors["kind.usesPerOrder"])}
          width={200}
        />
      </VStack>
    </Card>
  );
}

function tierLabel(i: number, count: number): string {
  return count === 1 ? "Tier" : `Tier ${i + 1}`;
}

export function TieredValue({
  draft,
  kind,
  errors,
  onChange,
  isReadOnly,
  markets,
}: KindProps<TieredDraft>) {
  const set = (change: Partial<TieredDraft>, paths: string[]) =>
    onChange({ ...draft, kind: { ...kind, ...change } }, paths);
  const setTier = (i: number, tier: TierDraft, field: string) =>
    set({ tiers: kind.tiers.map((t, j) => (j === i ? tier : t)) }, [`kind.tiers.${i}.${field}`]);
  const addTier = () =>
    set(
      {
        tiers: [
          ...kind.tiers,
          { quantity: "", amounts: {}, value: { ...emptyValue(), percent: "" } },
        ],
      },
      ["kind.tiers"],
    );
  const removeTier = (i: number) =>
    set({ tiers: kind.tiers.filter((_, j) => j !== i) }, ["kind.tiers"]);
  const tiersError = errors["kind.tiers"];

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Tiers
          </Text>
          <Text type="supporting" color="secondary">
            The highest tier the cart reaches applies to every item it counts. Each tier needs more
            than the one before it.
          </Text>
        </VStack>
        <SegmentedControl
          label="Counted by"
          value={kind.measure}
          onChange={(measure) =>
            set({ measure: measure === "subtotal" ? "subtotal" : "quantity" }, ["kind.tiers"])
          }
          isDisabled={isReadOnly}
        >
          <SegmentedControlItem value="quantity" label="Items in the cart" />
          <SegmentedControlItem value="subtotal" label="Amount spent" />
        </SegmentedControl>
        {tiersError && <Banner status="error" title={tiersError} />}
        {kind.tiers.map((tier, i) => (
          // Tiers have no identity of their own: their place is what they are.
          // oxlint-disable-next-line react/no-array-index-key -- tiers are addressed by position, like their error paths
          <VStack key={i} gap={3}>
            <Divider />
            <HStack gap={2} hAlign="between" vAlign="center">
              <Text weight="semibold">{tierLabel(i, kind.tiers.length)}</Text>
              {!isReadOnly && kind.tiers.length > 1 && (
                <Button label="Remove" variant="ghost" size="sm" onClick={() => removeTier(i)} />
              )}
            </HStack>
            {kind.measure === "quantity" ? (
              <TextInput
                label="From (items)"
                value={tier.quantity}
                onChange={(quantity) => setTier(i, { ...tier, quantity }, "quantity")}
                isDisabled={isReadOnly}
                status={fieldStatus(errors[`kind.tiers.${i}.quantity`])}
                width={200}
              />
            ) : (
              <AmountsByMarket
                label="From (spent)"
                amounts={tier.amounts}
                markets={markets}
                path={`kind.tiers.${i}.amounts`}
                errors={errors}
                onChange={(amounts) => setTier(i, { ...tier, amounts }, "amounts")}
                isReadOnly={isReadOnly}
              />
            )}
            <ValueFields
              value={tier.value}
              types={["percent", "amount"]}
              path={`kind.tiers.${i}.value`}
              markets={markets}
              errors={errors}
              onChange={(value) => setTier(i, { ...tier, value }, "value")}
              isReadOnly={isReadOnly}
            />
          </VStack>
        ))}
        {!isReadOnly && (
          <HStack>
            <Button
              label="Add tier"
              variant="secondary"
              onClick={addTier}
              isDisabled={kind.tiers.length >= MAX_TIERS}
            />
          </HStack>
        )}
      </VStack>
    </Card>
  );
}
