import { Divider } from "@astryxdesign/core/Divider";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { type Money, formatMoney } from "@/lib/money";

import { LineImage } from "./line-image";

function Row({ label, value, strong }: { label: string; value: string; strong?: boolean }) {
  return (
    <HStack hAlign="between" vAlign="center">
      <Text type={strong ? "large" : "body"} color={strong ? "primary" : "secondary"} weight={strong ? "semibold" : undefined}>
        {label}
      </Text>
      <Text type={strong ? "large" : "body"} weight={strong ? "semibold" : undefined} hasTabularNumbers>
        {value}
      </Text>
    </HStack>
  );
}

/** A cart's or an order's totals, with tax as the market shows it. */
export function Totals({
  subtotal,
  shipping,
  tax,
  total,
  taxesIncluded,
  digits,
}: {
  subtotal: Money;
  shipping: Money;
  tax: Money;
  total: Money;
  taxesIncluded: boolean;
  digits: number;
}) {
  return (
    <VStack gap={2}>
      <Row label="Subtotal" value={formatMoney(subtotal, digits)} />
      <Row label="Shipping" value={formatMoney(shipping, digits)} />
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
}: {
  quantity: number;
  title: string;
  variant?: string | null;
  total: string;
  /** The line's thumbnail URL, if it has an image. */
  image?: string | null;
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
        </VStack>
      </StackItem>
      <Text type="body" hasTabularNumbers>
        {total}
      </Text>
    </HStack>
  );
}
