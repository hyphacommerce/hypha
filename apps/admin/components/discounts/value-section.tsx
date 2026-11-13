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
import { useT } from "@/lib/use-t";

import { AmountsByMarket, type SectionProps, ValueFields } from "./fields";
import { BuyXGetYValue, TieredValue } from "./kind-value";

export function ValueSection({
  draft,
  errors,
  onChange,
  isReadOnly,
  markets,
}: SectionProps & { markets: readonly MarketOption[] }) {
  const t = useT<"discounts">();
  const kind = draft.kind;
  const priced = amountMarkets(draft.conditions, markets);

  if (kind.type === "buy_x_get_y") {
    return (
      <BuyXGetYValue
        draft={draft}
        kind={kind}
        errors={errors}
        onChange={onChange}
        isReadOnly={isReadOnly}
        markets={priced}
      />
    );
  }
  if (kind.type === "tiered") {
    return (
      <TieredValue
        draft={draft}
        kind={kind}
        errors={errors}
        onChange={onChange}
        isReadOnly={isReadOnly}
        markets={priced}
      />
    );
  }

  const set = (change: Partial<StandardDraft>, paths: string[]) =>
    onChange({ ...draft, kind: { ...kind, ...change } }, paths);

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("discounts.value_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("discounts.value_hint")}
          </Text>
        </VStack>
        {errors.kind && <Banner status="error" title={errors.kind} />}

        <Switch
          label={t("discounts.value_off_products")}
          description={t("discounts.value_off_products_hint")}
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
          label={t("discounts.value_off_order")}
          description={t("discounts.value_off_order_hint")}
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
          label={t("discounts.value_off_shipping")}
          description={t("discounts.value_off_shipping_hint")}
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
              label={t("discounts.value_cap")}
              placeholder={t("discounts.value_cap_placeholder")}
              description={t("discounts.value_cap_hint")}
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
