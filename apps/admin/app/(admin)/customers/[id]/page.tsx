import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { customerName, formatMoney } from "@/lib/format";
import { loadCountries, loadCurrencies, loaded, sessionClient } from "@/lib/hypha";

type Address = AdminSchemas["AddressResponse"];

/** An address as it would be written on a parcel. */
function AddressLines({ address, country }: { address: Address; country: string }) {
  // Each line by what it holds, which is also its key; blank ones are left out.
  const lines = Object.entries({
    name: [address.first_name, address.last_name].filter(Boolean).join(" "),
    company: address.company,
    line1: address.line1,
    line2: address.line2,
    city: [address.postal_code, address.city].filter(Boolean).join(" "),
    province: address.province,
    country,
    phone: address.phone,
  }).filter(([, line]) => line);
  return (
    <VStack gap={0.5}>
      {lines.map(([part, line], i) => (
        <Text key={part} color={i === 0 ? "primary" : "secondary"}>
          {line}
        </Text>
      ))}
    </VStack>
  );
}

export default async function CustomerPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const [customer, countries, currencies] = await Promise.all([
    client.GET("/admin/customers/{id}", { params: { path: { id } } }).then(loaded),
    loadCountries(),
    loadCurrencies(),
  ]);
  // Their latest orders: the email search matches parts of addresses too, so keep the
  // ones with exactly theirs. Needs `orders:read`; without it the card says so.
  const recent = await client.GET("/admin/orders", {
    params: { query: { q: customer.email, limit: 10 } },
  });
  const orders = (recent.data?.items ?? []).filter((o) => o.customer === customer.id);
  const countryName = new Map(countries.map((c) => [c.code, c.name]));
  const name = customerName(customer);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <PageHeader
          title={name || customer.email}
          back={{ label: "Customers", href: "/customers" }}
        />
        <div className="detail-columns">
          <div>
            <Card>
              <VStack gap={3}>
                <HStack hAlign="between" vAlign="center">
                  <Text type="display-3" as="h2">
                    Orders
                  </Text>
                  {orders.length > 0 && (
                    <Link href={`/orders?q=${encodeURIComponent(customer.email)}`}>View all</Link>
                  )}
                </HStack>
                {!recent.data ? (
                  <Text color="secondary">Reading orders needs the orders:read scope.</Text>
                ) : orders.length === 0 ? (
                  <Text color="secondary">No orders yet.</Text>
                ) : (
                  <VStack gap={0}>
                    {orders.map((o, i) => (
                      <div key={o.id} className={i > 0 ? "list-row list-row-divided" : "list-row"}>
                        <Link href={`/orders/${o.id}`} weight="medium">
                          #{o.number}
                        </Link>
                        <Text color="secondary">
                          <DateTime value={o.placed_at} short />
                        </Text>
                        <HStack gap={1} wrap="wrap">
                          {o.cancelled_at ? (
                            <CancelledBadge />
                          ) : (
                            <>
                              <PaymentBadge status={o.financial_status} />
                              <FulfillmentBadge status={o.fulfillment_status} />
                            </>
                          )}
                        </HStack>
                        <Text hasTabularNumbers>{formatMoney(o.total, currencies)}</Text>
                      </div>
                    ))}
                  </VStack>
                )}
              </VStack>
            </Card>
          </div>
          <div>
            <Card>
              <VStack gap={3}>
                <Text type="display-3" as="h2">
                  Details
                </Text>
                <MetadataList>
                  <MetadataListItem label="Email">{customer.email}</MetadataListItem>
                  <MetadataListItem label="Name">{name || "—"}</MetadataListItem>
                  <MetadataListItem label="Phone">{customer.phone ?? "—"}</MetadataListItem>
                  <MetadataListItem label="Customer since">
                    <DateTime value={customer.created_at} />
                  </MetadataListItem>
                  <MetadataListItem label="Last changed">
                    <DateTime value={customer.updated_at} />
                  </MetadataListItem>
                </MetadataList>
              </VStack>
            </Card>
            <Card>
              <VStack gap={3}>
                <Text type="display-3" as="h2">
                  Addresses
                </Text>
                {customer.addresses.length === 0 ? (
                  <Text color="secondary">
                    No saved addresses. Checkout keeps an order&apos;s address on the order.
                  </Text>
                ) : (
                  <VStack gap={4}>
                    {customer.addresses.map((address) => (
                      <VStack key={address.id} gap={2}>
                        <HStack gap={1}>
                          {customer.default_shipping_address === address.id && (
                            <Badge label="Default shipping" />
                          )}
                          {customer.default_billing_address === address.id && (
                            <Badge label="Default billing" />
                          )}
                        </HStack>
                        <AddressLines
                          address={address}
                          country={countryName.get(address.country) ?? address.country}
                        />
                      </VStack>
                    ))}
                  </VStack>
                )}
              </VStack>
            </Card>
          </div>
        </div>
      </VStack>
    </Section>
  );
}
