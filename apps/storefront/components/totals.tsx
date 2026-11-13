import { Divider } from "@astryxdesign/core/Divider";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

import { type Discounts, NO_DISCOUNTS, discountedTotals, minus } from "@/lib/discounts";
import type { TFor } from "@/lib/messages";
import { type Money, formatMoney } from "@/lib/money";

import { DiscountNotes } from "./discounts";
import { LineImage } from "./line-image";

function Row({ label, value, strong }: { label: string; value: string; strong?: boolean }) {
  return (
    <HStack hAlign="between" vAlign="center">
      <Text
        type={strong ? "large" : "body"}
        color={strong ? "primary" : "secondary"}
        weight={strong ? "semibold" : undefined}
      >
        {label}
      </Text>
      <Text
        type={strong ? "large" : "body"}
        weight={strong ? "semibold" : undefined}
        hasTabularNumbers
      >
        {value}
      </Text>
    </HStack>
  );
}

/** A cart's or an order's totals, with tax as the market shows it. With discounts, the
 * subtotal is before what came off the goods, a "Discounts" row takes it off, and
 * shipping a discount took all of is "Free". */
export function Totals({
  t,
  subtotal,
  shipping,
  tax,
  total,
  taxesIncluded,
  digits,
  discounts = NO_DISCOUNTS,
}: {
  t: TFor<"cart">;
  subtotal: Money;
  shipping: Money;
  tax: Money;
  total: Money;
  taxesIncluded: boolean;
  digits: number;
  discounts?: Pick<Discounts, "total" | "shipping">;
}) {
  const shown = discountedTotals(subtotal, discounts);
  const freeShipping = (discounts.shipping?.amount ?? 0) > 0 && shipping.amount <= 0;
  const money = (amount: Money) => formatMoney(amount, digits, t.locale);
  return (
    <VStack gap={2}>
      <Row label={t("cart.subtotal")} value={money(shown.subtotal)} />
      {shown.goods && (
        <Row label={t("cart.discounts")} value={minus(shown.goods, digits, t.locale)} />
      )}
      <Row
        label={t("cart.shipping")}
        value={freeShipping ? t("cart.shipping_free") : money(shipping)}
      />
      <Row label={taxesIncluded ? t("cart.including_tax") : t("cart.tax")} value={money(tax)} />
      <Divider />
      <Row label={t("cart.total")} value={money(total)} strong />
    </VStack>
  );
}

/** One line of a summary: "2 × Mug (M / Blue)" and its total. */
export function SummaryLine({
  t,
  quantity,
  title,
  variant,
  total,
  image,
  notes,
}: {
  t: TFor<"cart">;
  quantity: number;
  title: string;
  variant?: string | null;
  /** Formatted, or a `Price` (a sale's struck-through compare-at beside it). */
  total: string | ReactNode;
  /** The line's thumbnail URL, if it has an image. */
  image?: string | null;
  /** What discounts took off it ("−€2.40 Welcome offer"). */
  notes?: readonly string[];
}) {
  return (
    <HStack gap={3} hAlign="between" vAlign="start">
      <LineImage image={image} size={48} />
      <StackItem size="fill">
        <VStack gap={0.5}>
          <Text type="body" weight="medium">
            {t("cart.line", { quantity, title })}
          </Text>
          {variant && <Text type="supporting">{variant}</Text>}
          <DiscountNotes notes={notes} />
        </VStack>
      </StackItem>
      {typeof total === "string" ? (
        <Text type="body" hasTabularNumbers>
          {total}
        </Text>
      ) : (
        total
      )}
    </HStack>
  );
}
