// A price, with its compare-at struck through beside it when there's one. A screen
// reader hears "Was €40.00, now €30.00" rather than two bare amounts (a line through
// a number isn't read out), so the visible amounts are hidden from it. No hooks: server
// and client components both render it.

import { Badge } from "@astryxdesign/core/Badge";
import { HStack } from "@astryxdesign/core/Stack";
import { Text, type TextType } from "@astryxdesign/core/Text";
import { VisuallyHidden } from "@astryxdesign/core/VisuallyHidden";

import { type Money, formatMoney } from "@/lib/money";
import { saleLabel, shownCompareAt } from "@/lib/prices";

export function Price({
  price,
  compareAt,
  digits,
  text,
  type = "body",
  weight,
  color,
}: {
  price: Money;
  compareAt?: Money | null;
  digits: number;
  /** The price as shown, when it isn't `price` alone (a card's range). */
  text?: string;
  type?: TextType;
  weight?: "medium" | "semibold";
  color?: "primary" | "secondary";
}) {
  const now = text ?? formatMoney(price, digits);
  const was = shownCompareAt(price, compareAt);
  if (!was) {
    return (
      <Text type={type} weight={weight} color={color} hasTabularNumbers>
        {now}
      </Text>
    );
  }
  const wasText = formatMoney(was, digits);
  return (
    <span>
      <VisuallyHidden>{saleLabel(wasText, now)}</VisuallyHidden>
      <HStack as="span" gap={2} vAlign="center" wrap="wrap" aria-hidden="true">
        <Text type={type} weight={weight} color="primary" hasTabularNumbers>
          {now}
        </Text>
        <Text type={type} color="secondary" hasStrikethrough hasTabularNumbers>
          {wasText}
        </Text>
      </HStack>
    </span>
  );
}

/** "Sale", on what's shown with a compare-at price. */
export function SaleBadge() {
  return <Badge label="Sale" variant="error" />;
}
