import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import { ReloadCart } from "@/components/cart/cart-provider";
import { OrderDetails } from "@/components/order-details";
import { api, countryOf } from "@/lib/api";
import { link, orderTokenCookie } from "@/lib/site";

/** Per request: it reads the shopper's order with their token (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Order confirmed" };

type Props = {
  params: Promise<{ country: string; id: string }>;
  searchParams: Promise<{ token?: string | string[] }>;
};

/**
 * An order's confirmation (design note 0024, §7): read with the order's access token,
 * kept in an httpOnly cookie when it was placed here or opened from an email's link
 * (`/order/<id>?token=`, `app/order/[id]/route.ts`). Without the right token the order
 * isn't shown: its ID alone reads nothing.
 */
export default async function OrderConfirmedPage({ params, searchParams }: Props) {
  const { country: folder, id } = await params;
  // A token in this page's URL (an older link's shape): through the handler, which
  // keeps it in the cookie and comes back here without it.
  const given = (await searchParams).token;
  if (typeof given === "string" && given) {
    redirect(`/order/${encodeURIComponent(id)}?token=${encodeURIComponent(given)}`);
  }
  const country = await countryOf(folder);
  const code = country.country;
  const token = (await cookies()).get(orderTokenCookie(id))?.value;
  const order = token
    ? (
        await api().GET("/storefront/orders/{id}", {
          params: { path: { id }, query: { token } },
        })
      ).data
    : undefined;

  if (!order) {
    return (
      <EmptyState
        headingLevel={1}
        title="We can't show this order"
        description="Open it from the link in your order confirmation email."
        actions={<Button label="Back to the shop" href={link(code, "/")} variant="primary" />}
      />
    );
  }

  return (
    <VStack gap={8} maxWidth={960}>
      {/* The order was placed from the cart the header still counts: read it again. */}
      <ReloadCart />
      <VStack gap={2}>
        <HStack gap={3} vAlign="center" wrap="wrap">
          <Heading level={1} type="display-2">
            Thank you
          </Heading>
          <Badge label={`Order #${order.number}`} />
        </HStack>
        <Text type="large" color="secondary">
          Your order is placed. We'll write to {order.email} about it.
        </Text>
      </VStack>
      <OrderDetails
        order={order}
        country={country}
        after={
          <div>
            <Button label="Keep shopping" href={link(code, "/")} variant="primary" />
          </div>
        }
      />
    </VStack>
  );
}
