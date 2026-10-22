import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { AppSlot } from "@/components/app-extensions/app-slot";
import { CustomFieldsCard } from "@/components/custom-fields/custom-fields-card";
import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { customerName, formatMoney } from "@/lib/format";
import { loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import { loadCountries, loadCurrencies, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { wellKnownField, withoutWellKnown } from "@/lib/well-known-fields";

import { AccountCard } from "./account";
import { AddressesCard } from "./addresses";
import { ConsentCard } from "./consent";
import { DeleteCustomer } from "./delete-customer";
import { EditCustomer } from "./edit-customer";
import { EraseCustomer, ErasureBanner, ExportCustomer } from "./erasure";
import { GroupsCard } from "./groups-card";
import { CustomerLabels } from "./labels";
import { NoteCard } from "./note";

export default async function CustomerPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const [customer, countries, currencies, extensions, definitions, fields, me, groups] =
    await Promise.all([
      client.GET("/admin/customers/{id}", { params: { path: { id } } }).then(loaded),
      loadCountries(),
      loadCurrencies(),
      loadExtensions(),
      // Its custom fields: none shown without `custom_fields:read`.
      definitionsOf("customer"),
      client.GET("/admin/customers/{id}/custom-fields", { params: { path: { id } } }),
      requireStaff(),
      // The store's groups, for the groups card.
      client.GET("/admin/customer-groups").then((r) => r.data?.items ?? []),
    ]);
  // Their latest orders. Needs `orders:read`; without it the card says so.
  const recent = await client.GET("/admin/orders", {
    params: { query: { customer: customer.id, limit: 10 } },
  });
  const orders = recent.data?.items ?? [];
  const stats = customer.orders;
  const name = customerName(customer);
  const isErased = Boolean(customer.erased_at);
  const canWrite = hasScope(me.scopes, "customers:write") && !isErased;
  const canErase = hasScope(me.scopes, "customers:erase");
  const canDefine = hasScope(me.scopes, "custom_fields:write");
  // The well-known fields are rendered in place: labels under the name, the note as
  // its own card; the rest on the custom fields card.
  const labels = wellKnownField(definitions, "labels");
  const note = wellKnownField(definitions, "note");
  const otherFields = withoutWellKnown(definitions);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <PageHeader
          title={isErased ? "Erased customer" : name || customer.email}
          back={{ label: "Customers", href: "/customers" }}
          badges={
            <CustomerLabels
              customerId={customer.id}
              definition={labels}
              fields={fields.data}
              canEdit={canWrite}
              canDefine={canDefine}
            />
          }
          actions={
            <HStack gap={2}>
              <ExportCustomer id={customer.id} />
              <EraseCustomer
                id={customer.id}
                name={name || customer.email}
                canErase={canErase}
                erasureDueAt={customer.erasure_due_at}
                isErased={isErased}
              />
              {canWrite && (
                <DeleteCustomer
                  id={customer.id}
                  name={name || customer.email}
                  canDelete={customer.can_delete}
                />
              )}
              {canWrite && <EditCustomer customer={customer} />}
            </HStack>
          }
        />
        <ErasureBanner
          id={customer.id}
          erasureDueAt={customer.erasure_due_at}
          erasedAt={customer.erased_at}
          canErase={canErase}
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
            <NoteCard
              customerId={customer.id}
              definition={note}
              fields={fields.data}
              canEdit={canWrite}
              canDefine={canDefine}
            />
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
            <AccountCard
              customerId={customer.id}
              account={customer.account}
              canEdit={hasScope(me.scopes, "customers:write")}
            />
            <GroupsCard
              customerId={customer.id}
              groups={groups}
              current={customer.groups}
              canEdit={canWrite}
            />
            <ConsentCard
              customerId={customer.id}
              email={customer.email_marketing}
              sms={customer.sms_marketing}
              hasPhone={Boolean(customer.phone)}
              canEdit={canWrite}
              isErased={Boolean(customer.erased_at)}
            />
            <AddressesCard
              customer={customer.id}
              addresses={customer.addresses}
              defaultShipping={customer.default_shipping_address}
              defaultBilling={customer.default_billing_address}
              countries={countries}
              canWrite={canWrite}
            />
            {otherFields.length > 0 && fields.data && (
              <CustomFieldsCard
                ownerType="customer"
                ownerId={customer.id}
                definitions={otherFields}
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
