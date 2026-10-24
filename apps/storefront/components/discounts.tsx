// What discounts look like wherever they show: a line's notes ("−€2.40 Welcome offer"),
// a cart's codes with what their state means, the discounts an order got, and the
// promotion badges on products. No hooks: server and client components both render
// them, and each renders nothing without discounts.

import { Badge } from "@astryxdesign/core/Badge";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import {
  type Application,
  type CartCode,
  type Promotion,
  codeMessage,
  minus,
  promotionBadges,
} from "@/lib/discounts";

/** Under a line: what each discount took off it. */
export function DiscountNotes({ notes }: { notes?: readonly string[] }) {
  if (!notes || notes.length === 0) return null;
  return (
    <VStack gap={0.5}>
      {notes.map((note) => (
        <Text key={note} type="supporting" color="accent">
          {note}
        </Text>
      ))}
    </VStack>
  );
}

/** A cart's codes and their states, read only (the checkout's summary). */
export function CodeStates({
  codes,
  applications,
  digits,
}: {
  codes: readonly CartCode[];
  applications: Application[];
  digits: number;
}) {
  if (codes.length === 0) return null;
  return (
    <VStack as="ul" gap={1} aria-label="Discount codes" className="shop-plain-list">
      {codes.map((code) => {
        const { text, tone } = codeMessage(code, applications, digits);
        return (
          <li key={code.code}>
            <Text type="supporting" color={tone === "success" ? "accent" : "secondary"}>
              <Text as="span" type="supporting" weight="semibold">
                {code.code}
              </Text>
              {`: ${text}`}
            </Text>
          </li>
        );
      })}
    </VStack>
  );
}

/** The discounts an order or a cart got, each with its amount: under the totals. */
export function AppliedDiscounts({
  applications,
  digits,
}: {
  applications: readonly Application[];
  digits: number;
}) {
  if (applications.length === 0) return null;
  return (
    <VStack as="ul" gap={1} aria-label="Discounts applied" className="shop-plain-list">
      {applications.map((a) => (
        <li key={a.index}>
          <HStack gap={2} hAlign="between" vAlign="start">
            <VStack gap={0}>
              <Text type="supporting" weight="medium">
                {a.code ? `${a.title} (${a.code})` : a.title}
              </Text>
              {a.summary && (
                <Text type="supporting" color="secondary">
                  {a.summary}
                </Text>
              )}
            </VStack>
            <Text type="supporting" color="accent" hasTabularNumbers>
              {minus(a.amount, digits)}
            </Text>
          </HStack>
        </li>
      ))}
    </VStack>
  );
}

/** A product's promotions as badges ("Buy 2, get 1 free"): never a price, so static
 * pages stay right for everyone. */
export function PromotionBadges({ promotions }: { promotions: readonly Promotion[] }) {
  const labels = promotionBadges(promotions);
  if (labels.length === 0) return null;
  return (
    <>
      {labels.map((label) => (
        <Badge key={label} label={label} variant="success" />
      ))}
    </>
  );
}
