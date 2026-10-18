"use client";

// The cart page's body (design note 0024, §7): the same lines as the drawer, with the
// totals and the way to checkout beside them.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { Skeleton } from "@/components/skeletons";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";

import { Totals } from "@/components/totals";

import { CartLines } from "./cart-lines";
import { useCart } from "./cart-provider";

export function CartView() {
  const { cart, isLoading, digits, base, error } = useCart();
  if (isLoading) {
    return (
      <VStack gap={3}>
        {[0, 1, 2].map((i) => (
          <Skeleton key={i} index={i} height={88} />
        ))}
      </VStack>
    );
  }
  if (!cart || cart.lines.length === 0) {
    return (
      <EmptyState
        title="Your cart is empty"
        description="Find something you like and add it here."
        actions={<Button label="Keep shopping" href={`${base}/`} variant="primary" />}
      />
    );
  }
  return (
    <HStack gap={6} vAlign="start" wrap="wrap">
      <StackItem size="fill">
        <Card padding={5}>
          <VStack gap={2}>
            {error && <Banner status="error" title={error} />}
            <CartLines />
          </VStack>
        </Card>
      </StackItem>
      <VStack width={360} gap={4}>
        <Card padding={5}>
          <VStack gap={4}>
            <Heading level={2}>Summary</Heading>
            <Totals
              subtotal={cart.subtotal}
              shipping={cart.shipping_total}
              tax={cart.tax_total}
              total={cart.total}
              taxesIncluded={cart.taxes_included}
              digits={digits}
            />
            {cart.fully_available ? (
              <Button
                label="Check out"
                href={`${base}/checkout`}
                variant="primary"
                size="lg"
                width="100%"
              />
            ) : (
              <Banner status="warning" title="Remove what can't be bought to check out." />
            )}
            <Button label="Keep shopping" href={`${base}/`} variant="ghost" width="100%" />
          </VStack>
        </Card>
      </VStack>
    </HStack>
  );
}
