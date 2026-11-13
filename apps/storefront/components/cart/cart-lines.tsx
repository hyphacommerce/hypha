"use client";

// A cart's lines, shared by the drawer and the cart page: the picture, the product
// (linked), its variant, the quantity, Remove, and the line's total (after discounts,
// with what each took off under the variant and the total before them struck through
// beside it; else its compare-at total, when it's on sale). An unavailable line
// says why; it's left out of the totals.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Divider } from "@astryxdesign/core/Divider";
import { Link } from "@astryxdesign/core/Link";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { useEffect, useRef } from "react";

import type { Cart } from "@/app/[country]/actions";
import { LineImage, lineImage } from "@/components/line-image";
import { CartMessages } from "@/components/cart-messages";
import { DiscountNotes } from "@/components/discounts";
import { Price } from "@/components/price";
import { type Application, type LineDiscount, linePrice, readCartDiscounts } from "@/lib/discounts";
import { placeMessages } from "@/lib/cart-messages";
import { useT } from "@/lib/use-t";

import { useCart } from "./cart-provider";

type Line = Cart["lines"][number];

export function CartLines({ size = "md" }: { size?: "sm" | "md" }) {
  const { cart, digits, base, change, isPending } = useCart();
  const t = useT();
  if (!cart) return null;
  const discounts = readCartDiscounts(cart);
  const messages = placeMessages(cart.messages);
  return (
    <VStack gap={0}>
      {cart.lines.map((line, i) => (
        <VStack key={line.variant} gap={0}>
          {i > 0 && <Divider />}
          <CartLineRow
            line={line}
            discount={discounts.lines[line.variant]}
            applications={discounts.applications}
            digits={digits}
            href={line.handle ? `${base}/p/${line.handle}` : null}
            size={size}
            isPending={isPending}
            onChange={(quantity) => change(line.variant, quantity)}
          />
          <CartMessages t={t} messages={messages.lines[i]} />
        </VStack>
      ))}
    </VStack>
  );
}

function CartLineRow({
  line,
  discount,
  applications,
  digits,
  href,
  size,
  isPending,
  onChange,
}: {
  line: Line;
  discount: LineDiscount | undefined;
  applications: Application[];
  digits: number;
  href: string | null;
  size: "sm" | "md";
  isPending: boolean;
  onChange: (quantity: number) => void;
}) {
  const t = useT();
  const title = line.product_title ?? t("common.product_gone");
  // The quantity last asked for: the input reports a typed value and again on Enter or
  // blur, and a second write of the same value would race the first.
  const asked = useRef(line.quantity);
  useEffect(() => {
    asked.current = line.quantity;
  }, [line.quantity]);
  // The stock left (the API refuses more); `null` is no limit.
  const max = line.available_quantity ?? undefined;
  const ask = (quantity: number) => {
    if (quantity === asked.current) return;
    asked.current = quantity;
    onChange(quantity);
  };
  const shown =
    line.status === "priced" ? linePrice(t, line, discount, applications, digits) : null;
  return (
    <VStack gap={2} paddingBlock={size === "sm" ? 3 : 4}>
      <HStack gap={3} vAlign="start">
        <LineImage image={lineImage(line.image)} size={size === "sm" ? 56 : 80} />
        <StackItem size="fill">
          <VStack gap={1}>
            <HStack gap={2} hAlign="between" vAlign="start">
              <Text type="body" weight="semibold">
                {href ? (
                  <Link href={href} color="primary">
                    {title}
                  </Link>
                ) : (
                  title
                )}
              </Text>
              {shown ? (
                <Price
                  t={t}
                  price={shown.price}
                  compareAt={shown.compareAt}
                  digits={digits}
                  weight="medium"
                />
              ) : (
                <Text type="body" weight="medium">
                  —
                </Text>
              )}
            </HStack>
            {line.variant_title && <Text type="supporting">{line.variant_title}</Text>}
            {shown && <DiscountNotes notes={shown.notes} />}
            <HStack gap={2} vAlign="center">
              <div style={{ width: 104 }}>
                <NumberInput
                  label={t("cart.quantity_of", { title })}
                  isLabelHidden
                  size="sm"
                  value={line.quantity}
                  min={1}
                  max={max === undefined ? undefined : Math.max(1, max)}
                  isIntegerOnly
                  isDisabled={isPending}
                  onChange={(q) => {
                    if (q) ask(max === undefined ? q : Math.max(1, Math.min(q, max)));
                  }}
                />
              </div>
              <Button
                label={t("common.remove")}
                variant="ghost"
                size="sm"
                isDisabled={isPending}
                clickAction={() => ask(0)}
              />
              {max !== undefined && max <= 5 && line.status === "priced" && (
                <Text type="supporting">
                  {line.quantity >= max
                    ? t("cart.no_more_in_stock")
                    : t("cart.in_stock", { count: max })}
                </Text>
              )}
            </HStack>
          </VStack>
        </StackItem>
      </HStack>
      {line.status === "unavailable" && <Banner status="warning" title={line.reason} />}
    </VStack>
  );
}
