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
import { api, countryOf, pageLocale } from "@/lib/api";
import { pageT } from "@/lib/i18n";
import { segmentFor } from "@/lib/routing";
import { link, orderTokenCookie } from "@/lib/site";

/** Per request: it reads the shopper's order with their token. */
export const dynamic = "force-dynamic";

type Props = {
  params: Promise<{ country: string; id: string }>;
  searchParams: Promise<{ token?: string | string[] }>;
};

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("order.confirmed_meta_title") };
}

/**
 * An order's confirmation: read with the order's access token, kept in an httpOnly
 * cookie when it was placed here or opened from an email's link (`/order/<id>?token=`,
 * `app/order/[id]/route.ts`). Without the right token the order isn't shown: its ID
 * alone reads nothing.
 */
export default async function OrderConfirmedPage({ params, searchParams }: Props) {
  const { country: folder, id } = await params;
  // A token in this page's URL (an older link's shape): through the handler, which
  // keeps it in the cookie and comes back here without it.
  const given = (await searchParams).token;
  if (typeof given === "string" && given) {
    redirect(`/order/${encodeURIComponent(id)}?token=${encodeURIComponent(given)}`);
  }
  const [country, place, t] = await Promise.all([
    countryOf(folder),
    pageLocale(folder),
    pageT(folder),
  ]);
  const token = (await cookies()).get(orderTokenCookie(id))?.value;
  const order = token
    ? (
        await api().GET("/storefront/orders/{id}", {
          params: { path: { id }, query: { token } },
        })
      ).data
    : undefined;
  // The order reads in its own language whatever the URL says: the page goes where
  // its words are in that language too.
  const there = order && segmentFor(order.language, country, place.segment);
  if (there) redirect(link(there, `/order/${encodeURIComponent(id)}/confirmed`));

  if (!order) {
    return (
      <EmptyState
        headingLevel={1}
        title={t("order.cant_show_title")}
        description={t("order.cant_show_description")}
        actions={
          <Button label={t("common.back_to_shop")} href={link(folder, "/")} variant="primary" />
        }
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
            {t("order.thank_you")}
          </Heading>
          <Badge label={t("order.number", { number: order.number })} />
        </HStack>
        <Text type="large" color="secondary">
          {t("order.placed_note", { email: order.email })}
        </Text>
      </VStack>
      <OrderDetails
        t={t}
        order={order}
        country={country}
        after={
          <div>
            <Button label={t("common.keep_shopping")} href={link(folder, "/")} variant="primary" />
          </div>
        }
      />
    </VStack>
  );
}
