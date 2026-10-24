import { Divider } from "@astryxdesign/core/Divider";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

import { type Discounts, NO_DISCOUNTS, discountedTotals, minus } from "@/lib/discounts";
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
  subtotal,
  shipping,
  tax,
  total,
  taxesIncluded,
  digits,
  discounts = NO_DISCOUNTS,
}: {
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
  return (
    <VStack gap={2}>
      <Row label="Subtotal" value={formatMoney(shown.subtotal, digits)} />
      {shown.goods && <Row label="Discounts" value={minus(shown.goods, digits)} />}
      <Row label="Shipping" value={freeShipping ? "Free" : formatMoney(shipping, digits)} />
      <Row label={taxesIncluded ? "Including tax" : "Tax"} value={formatMoney(tax, digits)} />
      <Divider />
      <Row label="Total" value={formatMoney(total, digits)} strong />
    </VStack>
  );
}

/** One line of a summary: "2 × Mug (M / Blue)" and its total. */
export function SummaryLine({
  quantity,
  title,
  variant,
  total,
  image,
  notes,
}: {
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
            {quantity} × {title}
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
