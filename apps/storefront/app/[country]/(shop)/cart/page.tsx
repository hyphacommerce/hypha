import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { CartView } from "@/components/cart/cart-view";
import { pageT } from "@/lib/i18n";

/** Per request, like the shopper's cart it shows. */
export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("cart.meta_title") };
}

/** The cart: the drawer's lines with the totals, then checkout. */
export default async function CartPage({ params }: { params: Promise<{ country: string }> }) {
  const t = await pageT((await params).country);
  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        {t("cart.title")}
      </Heading>
      <CartView />
    </VStack>
  );
}
