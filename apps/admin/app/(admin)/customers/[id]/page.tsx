import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { AppSlot } from "@/components/app-extensions/app-slot";
import { CustomFieldsCard } from "@/components/custom-fields/custom-fields-card";
import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { customerName, formatMoney } from "@/lib/format";
import { loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import { loadCountries, loadCurrencies, loaded, requireStaff, sessionClient } from "@/lib/hypha";

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
  const [customer, countries, currencies, extensions, definitions, fields, me] = await Promise.all([
    client.GET("/admin/customers/{id}", { params: { path: { id } } }).then(loaded),
    loadCountries(),
    loadCurrencies(),
    loadExtensions(),
    // Its custom fields (design note 0033): none shown without `custom_fields:read`.
    definitionsOf("customer"),
    client.GET("/admin/customers/{id}/custom-fields", { params: { path: { id } } }),
    requireStaff(),
  ]);
  // Their latest orders (design note 0032, §12 step 1). Needs `orders:read`; without it
  // the card says so.
  const recent = await client.GET("/admin/orders", {
    params: { query: { customer: customer.id, limit: 10 } },
  });
  const orders = recent.data?.items ?? [];
  const stats = customer.orders;
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
                    <Link href={`/orders?customer=${customer.id}`}>View all</Link>
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
                  <MetadataListItem label="Orders">
                    {stats.count === 0
                      ? "None"
                      : `${stats.count} · ${stats.amount_spent.map((m) => formatMoney(m, currencies)).join(" + ")}`}
                  </MetadataListItem>
                  {stats.first_placed_at && (
                    <MetadataListItem label="First order">
                      <DateTime value={stats.first_placed_at} />
                    </MetadataListItem>
                  )}
                  {stats.last_placed_at && stats.count > 1 && (
                    <MetadataListItem label="Last order">
                      <DateTime value={stats.last_placed_at} />
                    </MetadataListItem>
                  )}
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
            {definitions.length > 0 && fields.data && (
              <CustomFieldsCard
                ownerType="customer"
                ownerId={customer.id}
                definitions={definitions}
                fields={fields.data}
                scopes={me.scopes}
              />
            )}
            <AppSlot
              extensions={extensions}
              target="customer.details"
              context={{ customer_id: customer.id }}
            />
          </div>
        </div>
      </VStack>
    </Section>
  );
}
