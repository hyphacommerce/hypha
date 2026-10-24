"use client";

// What the discount takes off: one reward per class (products, the order, shipping), each
// with its value. Several rewards make one discount ("20% off cultures + free shipping").

import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { Divider } from "@astryxdesign/core/Divider";
import { VStack } from "@astryxdesign/core/Layout";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";

import { type StandardDraft, amountMarkets, emptyItems, emptyValue } from "@/lib/discounts/draft";
import type { MarketOption } from "@/lib/discounts/validate";

import { AmountsByMarket, type SectionProps, ValueFields } from "./fields";

const LATER: Record<string, string> = {
  buy_x_get_y: "Buy X get Y",
  tiered: "Volume",
};

export function ValueSection({
  draft,
  errors,
  onChange,
  isReadOnly,
  markets,
}: SectionProps & { markets: readonly MarketOption[] }) {
  const kind = draft.kind;
  const priced = amountMarkets(draft.conditions, markets);

  if (kind.type !== "standard") {
    return (
      <Card>
        <VStack gap={3}>
          <Text type="display-3" as="h2">
            Value
          </Text>
          <Banner
            status="info"
            title={`${LATER[kind.type] ?? kind.type} discounts can't be edited here yet.`}
          />
        </VStack>
      </Card>
    );
  }

  const set = (change: Partial<StandardDraft>, paths: string[]) =>
    onChange({ ...draft, kind: { ...kind, ...change } }, paths);

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Value
          </Text>
          <Text type="supporting" color="secondary">
            What it takes off. Product discounts come off lines first, order discounts off the
            subtotal after them, then shipping. Discounts of one kind that combine add up: 10% and
            20% take 30% off.
          </Text>
        </VStack>
        {errors.kind && <Banner status="error" title={errors.kind} />}

        <Switch
          label="Off products"
          description="A percentage or an amount off the items it applies to, or a fixed price for them."
          value={kind.product !== null}
          onChange={(on) =>
            set(
              {
                product: on
                  ? { items: emptyItems(), value: { ...emptyValue(), percent: "10" } }
                  : null,
              },
              ["kind"],
            )
          }
          isDisabled={isReadOnly}
        />
        {kind.product && (
          <ValueFields
            value={kind.product.value}
            types={["percent", "amount", "fixed_price"]}
            path="kind.product.value"
            markets={priced}
            errors={errors}
            onChange={(value) =>
              kind.product &&
              set({ product: { ...kind.product, value } }, ["kind.product.value", "kind"])
            }
            isReadOnly={isReadOnly}
            hasAllocation
          />
        )}

        <Divider />
        <Switch
          label="Off the order"
          description="Off the subtotal of the items, after product discounts."
          value={kind.order !== null}
          onChange={(on) =>
            set({ order: on ? { value: { ...emptyValue(), percent: "10" } } : null }, ["kind"])
          }
          isDisabled={isReadOnly}
        />
        {kind.order && (
          <ValueFields
            value={kind.order.value}
            types={["percent", "amount"]}
            path="kind.order.value"
            markets={priced}
            errors={errors}
            onChange={(value) => set({ order: { value } }, ["kind.order.value", "kind"])}
            isReadOnly={isReadOnly}
          />
        )}

        <Divider />
        <Switch
          label="Off shipping"
          description="Off the chosen shipping options. An order gets one shipping discount at most."
          value={kind.shipping !== null}
          onChange={(on) =>
            set(
              {
                shipping: on ? { value: { ...emptyValue(), percent: "100" }, maxAmount: {} } : null,
              },
              ["kind"],
            )
          }
          isDisabled={isReadOnly}
        />
        {kind.shipping && (
          <>
            <ValueFields
              value={kind.shipping.value}
              types={["percent", "amount"]}
              path="kind.shipping.value"
              markets={priced}
              errors={errors}
              onChange={(value) =>
                kind.shipping &&
                set({ shipping: { ...kind.shipping, value } }, ["kind.shipping.value", "kind"])
              }
              isReadOnly={isReadOnly}
              hasFree
            />
            <AmountsByMarket
              label="Up to"
              placeholder="No cap"
              description="The most it takes off shipping in each market; blank for no cap."
              amounts={kind.shipping.maxAmount}
              markets={priced}
              path="kind.shipping.maxAmount"
              errors={errors}
              onChange={(maxAmount) =>
                kind.shipping &&
                set({ shipping: { ...kind.shipping, maxAmount } }, ["kind.shipping.maxAmount"])
              }
              isReadOnly={isReadOnly}
            />
          </>
        )}
      </VStack>
    </Card>
  );
}
