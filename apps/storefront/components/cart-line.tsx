"use client";

// One line of the cart: its quantity changes as the shopper sets it, Remove takes it
// out, and an unavailable line says why (it's left out of the totals).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useRef, useState, useTransition } from "react";

import { setQuantity } from "@/app/[country]/actions";

export function CartLine({
  country,
  variant,
  title,
  variantTitle,
  quantity,
  total,
  problem,
}: {
  country: string;
  variant: string;
  title: string;
  variantTitle?: string | null;
  quantity: number;
  /** The line's total, formatted; none for an unavailable line. */
  total?: string;
  /** Why it can't be bought, for an unavailable line. */
  problem?: string | null;
}) {
  const router = useRouter();
  const [pending, start] = useTransition();
  const [error, setError] = useState<string>();
  // The quantity last asked for: the input commits on Enter and again on blur, and a
  // second write of the same value would race the first.
  const asked = useRef(quantity);
  const change = (next: number) => {
    if (next === asked.current) return;
    asked.current = next;
    start(async () => {
      const result = await setQuantity({ country, variant, quantity: next });
      setError(result?.error);
      if (result) asked.current = quantity;
      else router.refresh();
    });
  };

  return (
    <VStack gap={3} paddingBlock={4}>
      <HStack gap={4} vAlign="start" wrap="wrap">
        <StackItem size="fill">
          <VStack gap={1}>
            <Text type="body" weight="semibold">
              {title}
            </Text>
            {variantTitle && <Text type="supporting">{variantTitle}</Text>}
          </VStack>
        </StackItem>
        <HStack gap={2} vAlign="center">
          <div style={{ width: 96 }}>
            <NumberInput
              label={`Quantity of ${title}`}
              isLabelHidden
              value={quantity}
              min={1}
              isIntegerOnly
              isDisabled={pending}
              onChange={(q) => {
                if (q) change(q);
              }}
            />
          </div>
          <Button
            label="Remove"
            variant="ghost"
            size="sm"
            isLoading={pending}
            clickAction={() => change(0)}
          />
        </HStack>
        <div style={{ minWidth: 96, textAlign: "end" }}>
          <Text type="body" weight="medium" hasTabularNumbers>
            {total ?? "—"}
          </Text>
        </div>
      </HStack>
      {problem && <Banner status="warning" title={problem} />}
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}
