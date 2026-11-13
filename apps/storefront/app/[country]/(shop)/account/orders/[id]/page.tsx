import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound, redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { OrderDetails } from "@/components/order-details";
import { accountApi } from "@/lib/account";
import { countryOf, loadCountries } from "@/lib/api";
import { pageT } from "@/lib/i18n";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("account.order_meta_title") };
}

/** One of the customer's orders, as its confirmation shows it: the session is the proof. */
export default async function AccountOrderPage({
  params,
}: {
  params: Promise<{ country: string; id: string }>;
}) {
  const { country: folder, id } = await params;
  const [country, t] = await Promise.all([countryOf(folder), pageT(folder)]);
  const to = (path: string) => link(folder, path);
  const client = await accountApi();
  if (!client) redirect(to("/account"));
  const { data: order, response } = await client.GET("/storefront/account/orders/{id}", {
    params: { path: { id } },
  });
  if (!order) {
    if (response.status === 401) redirect(to("/account"));
    if (response.status === 404) notFound();
    throw new Error(`order failed with ${response.status}`);
  }
  // The order's own country prices it; shown in the shopper's folder regardless.
  const countries = await loadCountries();
  const orderCountry = countries.find((c) => c.country === order.country) ?? country;
  return (
    <AccountShell t={t} title={t("account.your_order")} current="/account/orders" link={to}>
      <VStack gap={6} maxWidth={960}>
        <HStack gap={3} vAlign="center" wrap="wrap">
          <Badge label={t("order.number", { number: order.number })} />
        </HStack>
        <OrderDetails
          t={t}
          order={order}
          country={orderCountry}
          after={
            <div>
              <Button
                label={t("account.all_orders")}
                href={to("/account/orders")}
                variant="secondary"
              />
            </div>
          }
        />
      </VStack>
    </AccountShell>
  );
}
