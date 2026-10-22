import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";

import { AccountShell } from "@/components/account/account-shell";
import { SignOutButtons } from "@/components/account/forms";
import { SignInForm } from "@/components/account/sign-in-form";
import { CustomFields } from "@/components/custom-fields";
import { PLACED_DATE } from "@/components/order-details";
import { SiteLink } from "@/components/site-link";
import { accountApi, loadAccount } from "@/lib/account";
import { countryOf, fieldContext, loadStore } from "@/lib/api";
import { presentFields } from "@/lib/custom-fields";
import { formatMoney } from "@/lib/money";
import { link } from "@/lib/site";

/** Per request: the shopper's own account. */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Account" };

/**
 * The account: the sign-in form without a session, otherwise the overview with the
 * latest orders and the way to each section.
 */
export default async function AccountPage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const to = (path: string) => link(code, path);
  const account = await loadAccount();
  if (!account) {
    return <SignInForm country={code} storeName={(await loadStore()).name} />;
  }
  const client = (await accountApi())!;
  const [orders, context] = await Promise.all([
    client.GET("/storefront/account/orders", { params: { query: { limit: 5 } } }),
    fieldContext(country, account.custom_fields),
  ]);
  const recent = orders.data?.items ?? [];
  const name = [account.first_name, account.last_name].filter(Boolean).join(" ");
  const fields = presentFields("customer", account.custom_fields, context);
  return (
    <AccountShell
      title={name ? `Hi ${account.first_name ?? name}` : "Your account"}
      current="/account"
      link={to}
    >
      <Grid columns={{ minWidth: 320, repeat: "fit" }} gap={6}>
        <Card padding={5}>
          <VStack gap={4}>
            <HStack hAlign="between" vAlign="center">
              <Heading level={2}>Recent orders</Heading>
              {recent.length > 0 && <SiteLink href={to("/account/orders")}>All orders</SiteLink>}
            </HStack>
            {recent.length === 0 ? (
              <Text color="secondary">No orders yet.</Text>
            ) : (
              <VStack gap={3}>
                {recent.map((o) => (
                  <HStack key={o.id} hAlign="between" vAlign="center" wrap="wrap" gap={2}>
                    <VStack gap={0}>
                      <SiteLink href={to(`/account/orders/${o.id}`)}>Order #{o.number}</SiteLink>
                      <Text type="supporting" color="secondary">
                        {PLACED_DATE.format(new Date(o.placed_at))}
                      </Text>
                    </VStack>
                    <HStack gap={2} vAlign="center">
                      {o.cancelled_at ? (
                        <Badge label="Cancelled" />
                      ) : (
                        <Badge label={o.fulfillment_status.replace(/_/g, " ")} />
                      )}
                      <Text>{formatMoney(o.total, country.minor_units)}</Text>
                    </HStack>
                  </HStack>
                ))}
              </VStack>
            )}
          </VStack>
        </Card>
        <VStack gap={4}>
          <Card padding={5}>
            <VStack gap={2}>
              <Heading level={2}>Details</Heading>
              <Text>{account.email}</Text>
              {name && <Text color="secondary">{name}</Text>}
              {account.phone && <Text color="secondary">{account.phone}</Text>}
              <SiteLink href={to("/account/profile")}>Edit profile</SiteLink>
            </VStack>
          </Card>
          {fields.length > 0 && (
            <Card padding={5}>
              <CustomFields entries={fields} columns="single" />
            </Card>
          )}
          <SignOutButtons country={code} />
        </VStack>
      </Grid>
    </AccountShell>
  );
}
