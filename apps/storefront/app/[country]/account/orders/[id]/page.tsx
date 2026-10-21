import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound, redirect } from "next/navigation";

import { AccountShell } from "@/components/account/account-shell";
import { OrderDetails } from "@/components/order-details";
import { accountApi } from "@/lib/account";
import { countryOf, loadCountries } from "@/lib/api";
import { link } from "@/lib/site";

export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Your order" };

/** One of the customer's orders, as its confirmation shows it: the session is the proof. */
export default async function AccountOrderPage({
  params,
}: {
  params: Promise<{ country: string; id: string }>;
}) {
  const { country: folder, id } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const to = (path: string) => link(code, path);
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
    <AccountShell title="Your order" current="/account/orders" link={to}>
      <VStack gap={6} maxWidth={960}>
        <HStack gap={3} vAlign="center" wrap="wrap">
          <Badge label={`Order #${order.number}`} />
        </HStack>
        <OrderDetails
          order={order}
          country={orderCountry}
          after={
            <div>
              <Button label="All orders" href={to("/account/orders")} variant="secondary" />
            </div>
          }
        />
      </VStack>
    </AccountShell>
  );
}
