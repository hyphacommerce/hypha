import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { statusLabel } from "@/components/order-details";
import { SiteLink } from "@/components/site-link";
import { accountApi } from "@/lib/account";
import { countryOf } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { pageT } from "@/lib/i18n";
import { formatMoney } from "@/lib/money";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("account.orders_meta_title") };
}

/** The customer's orders, newest first, a page at a time (`?cursor=`). */
export default async function OrdersPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<{ cursor?: string | string[] }>;
}) {
  const folder = (await params).country;
  const [country, t] = await Promise.all([countryOf(folder), pageT(folder)]);
  const to = (path: string) => link(folder, path);
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
    <AccountShell t={t} title={t("account.your_orders")} current="/account/orders" link={to}>
      {page.items.length === 0 ? (
        <Text color="secondary">{t("account.no_orders")}</Text>
      ) : (
        <VStack gap={3} maxWidth={760}>
          {page.items.map((o) => (
            <Card key={o.id} padding={4}>
              <HStack hAlign="between" vAlign="center" wrap="wrap" gap={3}>
                <VStack gap={0}>
                  <SiteLink href={to(`/account/orders/${o.id}`)}>
                    {t("order.number", { number: o.number })}
                  </SiteLink>
                  <Text type="supporting" color="secondary">
                    {t("account.order_summary", {
                      date: formatDate(o.placed_at, t.locale),
                      count: o.item_count,
                    })}
                  </Text>
                </VStack>
                <HStack gap={2} vAlign="center" wrap="wrap">
                  {o.cancelled_at ? (
                    <Badge label={t("order.cancelled")} />
                  ) : (
                    <>
                      <Badge label={statusLabel(t, "financial", o.financial_status)} />
                      <Badge label={statusLabel(t, "fulfillment", o.fulfillment_status)} />
                    </>
                  )}
                  <Text weight="bold">{formatMoney(o.total, country.minor_units, t.locale)}</Text>
                </HStack>
              </HStack>
            </Card>
          ))}
          {page.has_more && page.next_cursor && (
            <div>
              <Button
                label={t("account.older_orders")}
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
