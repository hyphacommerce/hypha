"use client";

// The cart in a drawer: a dialog against the page's end edge, opened from the header
// and after adding to the cart, with the lines, the discount code field, the subtotal
// (before discounts, as the cart page shows it, then what they took off and what's left)
// and the way on to the cart page and checkout.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog } from "@astryxdesign/core/Dialog";
import { Divider } from "@astryxdesign/core/Divider";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { readCartDiscounts, subtotalRows } from "@/lib/discounts";

import { CartLines } from "./cart-lines";
import { useCart } from "./cart-provider";
import { DiscountCodeField } from "./discount-code-field";

export function CartDrawer() {
  const { cart, count, digits, base, isOpen, close, error } = useCart();
  const isEmpty = !cart || cart.lines.length === 0;
  const discounts = readCartDiscounts(cart);
  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={(open) => !open && close()}
      position={{ top: 0, bottom: 0, end: 0 }}
      width={440}
      maxHeight="100dvh"
      style={{ height: "100dvh", borderRadius: 0 }}
      padding={5}
      aria-label="Cart"
    >
      <VStack gap={4} height="100%">
        <HStack hAlign="between" vAlign="center">
          <Heading level={2}>Cart{count > 0 ? ` (${count})` : ""}</Heading>
          <Button label="Close" variant="ghost" size="sm" clickAction={close} />
        </HStack>
        {error && <Banner status="error" title={error} />}
        {isEmpty ? (
          <EmptyState
            title="Your cart is empty"
            description="Find something you like and add it here."
            actions={<Button label="Keep shopping" variant="primary" clickAction={close} />}
          />
        ) : (
          <>
            <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
              <CartLines size="sm" />
            </div>
            <Divider />
            <VStack gap={3}>
              <DiscountCodeField discounts={discounts} />
              {subtotalRows(cart.subtotal, discounts, digits).map((row) => (
                <HStack key={row.label} hAlign="between">
                  <Text
                    color={row.strong ? undefined : "secondary"}
                    weight={row.strong ? "medium" : undefined}
                  >
                    {row.label}
                  </Text>
                  <Text weight={row.strong ? "semibold" : undefined} hasTabularNumbers>
                    {row.value}
                  </Text>
                </HStack>
              ))}
              <Text type="supporting" color="secondary">
                Shipping and taxes are worked out at checkout.
              </Text>
              <Button
                label="Check out"
                variant="primary"
                size="lg"
                href={`${base}/checkout`}
                isDisabled={!cart.fully_available}
              />
              <Button label="View cart" variant="secondary" href={`${base}/cart`} />
            </VStack>
          </>
        )}
      </VStack>
    </Dialog>
  );
}
