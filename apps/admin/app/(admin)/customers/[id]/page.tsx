import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Suspense } from "react";

import { AppSlot } from "@/components/app-extensions/app-slot";
import { CustomFieldsCard } from "@/components/custom-fields/custom-fields-card";
import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { customerName } from "@/lib/format";
import { getFormat } from "@/lib/get-format";
import { loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import { loadCountries, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { languageLabel } from "@/lib/languages";
import { hasScope } from "@/lib/navigation";
import type { AddressFormat } from "@/lib/address";
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

/** The way back to the list. */
function back(t: T): Crumb {
  return { label: t("common.customers"), href: "/customers" };
}

/** The scope the orders card needs: named as it is in every language. */
const ORDERS_SCOPE = "orders:read";

/**
 * A customer: their orders, note, details, account, groups, consent, addresses and
 * custom fields. The way back renders at once; the customer streams in behind a
 * skeleton of the page.
 */
export default async function CustomerPage({ params }: { params: Promise<{ id: string }> }) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={back(t)} />}>
          <Customer id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

/** Each country's address form, by country. */
async function addressFormatsOf(countries: string[]): Promise<Record<string, AddressFormat>> {
  const client = await sessionClient();
  const unique = [...new Set(countries)];
  const formats = await Promise.all(
    unique.map((country) =>
      client
        .GET("/admin/address-formats/{country}", { params: { path: { country } } })
        .then((r) => r.data),
    ),
  );
  return Object.fromEntries(
    formats.filter((f): f is AddressFormat => f !== undefined).map((f) => [f.country, f]),
  );
}

async function Customer({ id }: { id: string }) {
  const client = await sessionClient();
  const [customer, countries, format, extensions, definitions, fields, me, groups, t] =
    await Promise.all([
      client.GET("/admin/customers/{id}", { params: { path: { id } } }).then(loaded),
      loadCountries(),
      getFormat(),
      loadExtensions(),
      // Its custom fields: none shown without `custom_fields:read`.
      definitionsOf("customer"),
      client.GET("/admin/customers/{id}/custom-fields", { params: { path: { id } } }),
      requireStaff(),
      // The store's groups, for the groups card.
      client.GET("/admin/customer-groups").then((r) => r.data?.items ?? []),
      getT(),
    ]);
  // Their latest orders. Needs `orders:read`; without it the card says so.
  const recent = await client.GET("/admin/orders", {
    params: { query: { customer: customer.id, limit: 10 } },
  });
  const orders = recent.data?.items ?? [];
  // The forms of the countries in the address book, for editing those addresses.
  const formats = await addressFormatsOf(customer.addresses.map((a) => a.country));
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
    <>
      <PageHeader
        title={isErased ? t("customers.detail_erased_title") : name || customer.email}
        back={back(t)}
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
                  {t("common.orders")}
                </Text>
                {orders.length > 0 && (
                  <Link href={`/orders?customer=${customer.id}`}>{t("common.view_all")}</Link>
                )}
              </HStack>
              {!recent.data ? (
                <Text color="secondary">
                  {t("customers.detail_orders_needs_scope", { scope: ORDERS_SCOPE })}
                </Text>
              ) : orders.length === 0 ? (
                <Text color="secondary">{t("customers.detail_orders_empty")}</Text>
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
                          <CancelledBadge t={t} />
                        ) : (
                          <>
                            <PaymentBadge status={o.financial_status} t={t} />
                            <FulfillmentBadge status={o.fulfillment_status} t={t} />
                          </>
                        )}
                      </HStack>
                      <Text hasTabularNumbers>{format.money(o.total)}</Text>
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
                {t("common.details")}
              </Text>
              <MetadataList>
                <MetadataListItem label={t("common.email")}>{customer.email}</MetadataListItem>
                <MetadataListItem label={t("common.name")}>
                  {name || t("common.empty_value")}
                </MetadataListItem>
                <MetadataListItem label={t("common.phone")}>
                  {customer.phone ?? t("common.empty_value")}
                </MetadataListItem>
                <MetadataListItem label={t("common.language")}>
                  {customer.language
                    ? languageLabel(format, customer.language)
                    : t("customers.detail_language_default")}
                </MetadataListItem>
                <MetadataListItem label={t("common.orders")}>
                  {stats.count === 0
                    ? t("common.none")
                    : `${format.number(stats.count)} · ${stats.amount_spent.map((m) => format.money(m)).join(" + ")}`}
                </MetadataListItem>
                {stats.first_placed_at && (
                  <MetadataListItem label={t("customers.detail_first_order")}>
                    <DateTime value={stats.first_placed_at} />
                  </MetadataListItem>
                )}
                {stats.last_placed_at && stats.count > 1 && (
                  <MetadataListItem label={t("customers.detail_last_order")}>
                    <DateTime value={stats.last_placed_at} />
                  </MetadataListItem>
                )}
                <MetadataListItem label={t("customers.detail_since")}>
                  <DateTime value={customer.created_at} />
                </MetadataListItem>
                <MetadataListItem label={t("customers.detail_last_changed")}>
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
            formats={formats}
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
    </>
  );
}
