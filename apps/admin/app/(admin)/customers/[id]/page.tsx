import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";

import { PageHeader } from "@/components/page-header";
import { formatDateTime } from "@/lib/format";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { customerName } from "@/resources/customers";

type Address = AdminSchemas["AddressResponse"];

/** An address as it would be written on a parcel. */
function AddressLines({ address, country }: { address: Address; country: string }) {
  const lines = [
    [address.first_name, address.last_name].filter(Boolean).join(" "),
    address.company,
    address.line1,
    address.line2,
    [address.postal_code, address.city].filter(Boolean).join(" "),
    address.province,
    country,
    address.phone,
  ].filter(Boolean);
  return (
    <VStack gap={0.5}>
      {lines.map((line, i) => (
        <Text key={`${i}-${line}`} color={i === 0 ? "primary" : "secondary"}>
          {line}
        </Text>
      ))}
    </VStack>
  );
}

export default async function CustomerPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const [customer, countries] = await Promise.all([
    client.GET("/admin/customers/{id}", { params: { path: { id } } }).then(loaded),
    loadCountries(),
  ]);
  const countryName = new Map(countries.map((c) => [c.code, c.name]));
  const name = customerName(customer);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={name || customer.email}
          back={{ label: "Customers", href: "/customers" }}
          actions={
            <Button
              label="Their orders"
              variant="secondary"
              href={`/orders?q=${encodeURIComponent(customer.email)}`}
            />
          }
        />
        <Grid columns={{ minWidth: 320 }} gap={4}>
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
                  {formatDateTime(customer.created_at)}
                </MetadataListItem>
                <MetadataListItem label="Last changed">
                  {formatDateTime(customer.updated_at)}
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
                <Text color="secondary">No saved addresses. Checkout keeps an order&apos;s address on the order.</Text>
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
        </Grid>
      </VStack>
    </Section>
  );
}
