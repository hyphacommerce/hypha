import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { CartView } from "@/components/cart/cart-view";

/** Per request, like the shopper's cart it shows (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Cart" };

/** The cart (design note 0023, §3): the drawer's lines with the totals, then checkout. */
export default function CartPage() {
  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        Cart
      </Heading>
      <CartView />
    </VStack>
  );
}
