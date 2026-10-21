import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { PLACED_DATE } from "@/components/order-details";
import { SiteLink } from "@/components/site-link";
import { accountApi } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { formatMoney } from "@/lib/money";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Your orders" };

/** The customer's orders, newest first, a page at a time (`?cursor=`). */
export default async function OrdersPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<{ cursor?: string | string[] }>;
}) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const client = await accountApi();
  if (!client) redirect(to("/account"));
  const given = (await searchParams).cursor;
  const cursor = typeof given === "string" ? given : undefined;
  const { data: page, response } = await client.GET("/storefront/account/orders", {
    params: { query: { limit: 20, ...(cursor ? { cursor } : {}) } },
  });
  if (!page) {
    if (response.status === 401) redirect(to("/account"));
    throw new Error(`orders failed with ${response.status}`);
  }
  return (
    <AccountShell title="Your orders" current="/account/orders" link={to}>
      {page.items.length === 0 ? (
        <Text color="secondary">No orders yet.</Text>
      ) : (
        <VStack gap={3} maxWidth={760}>
          {page.items.map((o) => (
            <Card key={o.id} padding={4}>
              <HStack hAlign="between" vAlign="center" wrap="wrap" gap={3}>
                <VStack gap={0}>
                  <SiteLink href={to(`/account/orders/${o.id}`)}>Order #{o.number}</SiteLink>
                  <Text type="supporting" color="secondary">
                    {PLACED_DATE.format(new Date(o.placed_at))} · {o.item_count}{" "}
                    {o.item_count === 1 ? "item" : "items"}
                  </Text>
                </VStack>
                <HStack gap={2} vAlign="center" wrap="wrap">
                  {o.cancelled_at ? (
                    <Badge label="Cancelled" />
                  ) : (
                    <>
                      <Badge label={o.financial_status.replace(/_/g, " ")} />
                      <Badge label={o.fulfillment_status.replace(/_/g, " ")} />
                    </>
                  )}
                  <Text weight="bold">{formatMoney(o.total, country.minor_units)}</Text>
                </HStack>
              </HStack>
            </Card>
          ))}
          {page.has_more && page.next_cursor && (
            <div>
              <Button
                label="Older orders"
                variant="secondary"
                href={to(`/account/orders?cursor=${encodeURIComponent(page.next_cursor)}`)}
              />
            </div>
          )}
        </VStack>
      )}
    </AccountShell>
  );
}
