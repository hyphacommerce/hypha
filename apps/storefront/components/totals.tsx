import { Divider } from "@astryxdesign/core/Divider";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { type Money, formatMoney } from "@/lib/money";

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
}: {
  quantity: number;
  title: string;
  variant?: string | null;
  total: string;
}) {
  return (
    <HStack gap={3} hAlign="between" vAlign="start">
      <VStack gap={0.5}>
        <Text type="body" weight="medium">
          {quantity} × {title}
        </Text>
        {variant && <Text type="supporting">{variant}</Text>}
      </VStack>
      <Text type="body" hasTabularNumbers>
        {total}
      </Text>
    </HStack>
  );
}
