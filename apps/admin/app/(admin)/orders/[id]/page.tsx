import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { type ReactNode, Suspense } from "react";

import { Link } from "@astryxdesign/core/Link";

import { AppActions, AppSlot } from "@/components/app-extensions/app-slot";
import { CustomFieldsCard } from "@/components/custom-fields/custom-fields-card";
import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import {
  CancelledBadge,
  FulfillmentBadge,
  NeedsReviewBadge,
  PaymentBadge,
  PaymentStatusBadge,
} from "@/components/status";
import { cancelReasonLabel, unitCostTotal } from "@/lib/format";
import { type Format, getFormat } from "@/lib/get-format";
import {
  channelNames,
  loadCountries,
  loadStore,
  loaded,
  requireStaff,
  sessionClient,
  staffNames,
} from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { type Extensions, loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import { languageLabel } from "@/lib/languages";
import { canMarkPaid, providerLabel } from "@/lib/payments";
import { countryName } from "@/lib/regions";

import { AddressCheckCard, EditAddressButton } from "./address-check";
import { EmailsCard } from "./emails";
import { CancelOrderButton, FulfilButton, type Place, ShipmentsCard } from "./fulfilment";
import { MarkPaid } from "./mark-paid";

/** The way back to the list. */
function back(t: T): Crumb {
  return { label: t("common.orders"), href: "/orders" };
}

/** "25%" from parts per million (250000). */
function percent(format: Format, ppm: number): string {
  return format.percent(ppm / 1_000_000, { maximumFractionDigits: 4 });
}

/** A tax as the items table shows it: "€4.00 (25%)", or a dash without one. */
function taxCell(
  t: T,
  format: Format,
  tax: { amount: AdminSchemas["Money"]; rate_ppm: number } | null | undefined,
): string {
  return tax
    ? t("orders.detail_tax_with_rate", {
        amount: format.money(tax.amount),
        rate: percent(format, tax.rate_ppm),
      })
    : t("common.empty_value");
}

type OrderLine = AdminSchemas["OrderLineResponse"];

/**
 * A line's unit price, its compare-at struck through beside it when that's above what it
 * cost, and the price list that priced it (linked; by ID until its name is read, and
 * when it can't be).
 */
function UnitPrice({
  line,
  t,
  format,
  listNames,
}: {
  line: OrderLine;
  t: T;
  format: Format;
  listNames: Promise<Map<string, string>>;
}) {
  const was = line.compare_at_price;
  const isStruck =
    was && was.currency === line.unit_price.currency && was.amount > line.unit_price.amount;
  return (
    <VStack gap={0.5}>
      <HStack gap={2} wrap="wrap">
        <Text type="body" hasTabularNumbers>
          {format.money(line.unit_price)}
        </Text>
        {isStruck && (
          <Text type="body" color="secondary" hasStrikethrough hasTabularNumbers>
            {format.money(was)}
          </Text>
        )}
      </HStack>
      {line.unit_cost && <Margin line={line} cost={line.unit_cost} t={t} format={format} />}
      {line.price_list && (
        <Link href={`/markets/price-lists/${line.price_list}`}>
          <Text type="supporting">
            <Suspense fallback={line.price_list}>
              <ListName id={line.price_list} names={listNames} />
            </Suspense>
          </Text>
        </Link>
      )}
    </VStack>
  );
}

/**
 * What a unit cost the store when the order was placed, and the line's margin: what it
 * was charged less what its units cost, when both are in one currency.
 */
function Margin({
  line,
  cost,
  t,
  format,
}: {
  line: OrderLine;
  cost: AdminSchemas["UnitCost"];
  t: T;
  format: Format;
}) {
  const costs = unitCostTotal(cost, line.quantity);
  const margin =
    costs.currency === line.total.currency
      ? { amount: line.total.amount - costs.amount, currency: costs.currency }
      : null;
  // The margin's share of what the line was charged, to a whole percent.
  const share =
    margin && line.total.amount > 0 ? Math.round((margin.amount / line.total.amount) * 100) : null;
  const values = { cost: format.unitCost(cost) };
  return (
    <Text type="supporting" color="secondary" hasTabularNumbers>
      {margin === null
        ? t("orders.detail_line_cost", values)
        : share === null
          ? t("orders.detail_line_cost_margin", { ...values, margin: format.money(margin) })
          : t("orders.detail_line_cost_margin_share", {
              ...values,
              margin: format.money(margin),
              share: format.percent(share / 100, { maximumFractionDigits: 0 }),
            })}
    </Text>
  );
}

/** A price list's name once `names` is read; its ID when it isn't among them. */
async function ListName({ id, names }: { id: string; names: Promise<Map<string, string>> }) {
  return (await names).get(id) ?? id;
}

/**
 * The names of the price lists that priced `lines`, by ID: one read per list, none for
 * an order without. Without `pricing:read` (or when a read fails), none: the lines show
 * the lists by ID. Never awaited by the page, so the order shows without waiting.
 */
async function priceListNames(
  client: Awaited<ReturnType<typeof sessionClient>>,
  lines: readonly OrderLine[],
): Promise<Map<string, string>> {
  const ids = [...new Set(lines.flatMap((l) => (l.price_list ? [l.price_list] : [])))];
  const read = await Promise.all(
    ids.map((id) =>
      client
        .GET("/admin/price-lists/{id}", { params: { path: { id } } })
        .then((r) => (r.data ? ([id, r.data.name] as const) : null))
        .catch(() => null),
    ),
  );
  return new Map(read.filter((e) => e !== null));
}

type DiscountShare = AdminSchemas["DiscountShareResponse"];
type OrderDiscount = AdminSchemas["OrderDiscountResponse"];

/**
 * What a line or a shipping line was charged: its amount before discounts struck through
 * above it when discounts took something off, and each discount's part under it
 * ("−€2.40 Welcome offer").
 */
function Charged({
  before,
  total,
  shares,
  discounts,
  t,
  format,
}: {
  before: AdminSchemas["Money"];
  total: AdminSchemas["Money"];
  shares: readonly DiscountShare[];
  /** The order's discounts, which `shares` name by index. */
  discounts: readonly OrderDiscount[];
  t: T;
  format: Format;
}) {
  if (shares.length === 0) return <>{format.money(total)}</>;
  return (
    <VStack gap={0.5}>
      <Text type="supporting" color="secondary" hasStrikethrough hasTabularNumbers>
        {format.money(before)}
      </Text>
      <Text type="body" hasTabularNumbers>
        {format.money(total)}
      </Text>
      {shares.map((share) => {
        const values = {
          amount: format.money(share.amount),
          title: discounts[share.application]?.title ?? t("common.discount"),
        };
        return (
          <Text key={share.application} type="supporting" color="secondary">
            {share.units
              ? t("orders.detail_discount_share_units", { ...values, count: share.units })
              : t("orders.detail_discount_share", values)}
          </Text>
        );
      })}
    </VStack>
  );
}

/** The discounts an order got, one row per reward, with what each took off. */
function DiscountsList({ order, t, format }: { order: Order; t: T; format: Format }) {
  if (order.discounts.length === 0) return null;
  return (
    <VStack gap={2}>
      <HStack gap={2} hAlign="between" vAlign="center">
        <Text weight="semibold">{t("orders.detail_discounts")}</Text>
        <Text weight="semibold" hasTabularNumbers>
          −{format.money(order.discount_total)}
        </Text>
      </HStack>
      {order.discounts.map((d) => (
        // A discount gives at most one reward per class.
        <HStack key={`${d.discount}-${d.class}`} gap={2} hAlign="between" vAlign="start">
          <VStack gap={0}>
            <Link href={`/discounts/${d.discount}`}>
              <Text type="body" weight="medium">
                {d.title}
              </Text>
            </Link>
            {d.localized && (
              <Text type="supporting" color="secondary">
                {t("orders.detail_discount_localized", { title: d.localized.title })}
              </Text>
            )}
            <Text type="supporting" color="secondary">
              {[
                d.code
                  ? t("orders.detail_discount_code", { code: d.code })
                  : t("orders.detail_discount_automatic"),
                d.summary,
              ].join(" · ")}
            </Text>
          </VStack>
          <Text hasTabularNumbers>−{format.money(d.amount)}</Text>
        </HStack>
      ))}
      <Text type="supporting" color="secondary">
        {t("orders.detail_discounts_note")}
      </Text>
    </VStack>
  );
}

/** A footer row of the items table: its label, and the amount in the last column. */
function TotalRow({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <TableRow>
      <TableCell>{label}</TableCell>
      <TableCell />
      <TableCell />
      <TableCell />
      <TableCell />
      <TableCell>{children}</TableCell>
    </TableRow>
  );
}

/**
 * An order: its items and totals, payment, shipments, emails and timeline beside its
 * summary, customer and address. The way back renders at once; the order streams in
 * behind a skeleton of the page.
 */
export default async function OrderPage({ params }: { params: Promise<{ id: string }> }) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={back(t)} />}>
          <Order id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Order({ id }: { id: string }) {
  const [client, t, format] = await Promise.all([sessionClient(), getT(), getFormat()]);
  // All at once; a missing order still goes to the 404 page (`loaded`). Shipments and
  // locations need their own scopes; without them the page shows the order alone. The
  // emails need `notifications:read` and `orders:read` (they name the buyer); without
  // them the request is refused and the card isn't shown. The staff member is the
  // layout's read too. What installed apps add here is one more read.
  const [found, shipments, locations, channelName, emails, me, extensions, definitions, fields] =
    await Promise.all([
      client.GET("/admin/orders/{id}", { params: { path: { id } } }),
      client.GET("/admin/orders/{id}/fulfillments", { params: { path: { id } } }),
      client.GET("/admin/locations"),
      channelNames(client),
      client.GET("/admin/notifications", { params: { query: { order: id } } }),
      requireStaff(),
      loadExtensions(),
      // Its custom fields: none shown without `custom_fields:read`.
      definitionsOf("order"),
      client.GET("/admin/orders/{id}/custom-fields", { params: { path: { id } } }),
    ]);
  const order = loaded(found);
  // The country's form, for changing the address: read once the order names its country.
  // The address check names an app and a staff member by ID: their names, when it does
  // (IDs stay where the role can't read apps or users).
  const check = order.address_check;
  const [countries, store, addressFormat, people, apps] = await Promise.all([
    loadCountries(),
    loadStore(),
    order.shipping_address
      ? client
          .GET("/admin/address-formats/{country}", {
            params: { path: { country: order.country } },
          })
          .then((r) => r.data)
      : undefined,
    check?.marked_valid ? staffNames(client) : new Map<string, string>(),
    check?.verification?.app
      ? client
          .GET("/admin/apps")
          .then((r) => new Map((r.data?.items ?? []).map((a) => [a.key, a.name])))
      : new Map<string, string>(),
  ]);
  const canWriteOrders = me.scopes.includes("orders:write");
  // The language the buyer read the order in, when it isn't the one content is written
  // in (none when the store can't be read: there's nothing to compare with).
  const buyerLanguage =
    order.language && store && order.language !== store.default_language ? order.language : null;
  const listNames = priceListNames(client, order.lines);

  const fulfillments = shipments.data?.items ?? [];
  const places: Place[] = (locations.data?.items ?? []).map((l) => ({ id: l.id, name: l.name }));
  const cancelled = order.cancelled_at !== null && order.cancelled_at !== undefined;
  const canFulfil =
    !cancelled && order.unfulfilled.length > 0 && places.length > 0 && shipments.data;

  return (
    <>
      <PageHeader
        // An order's number is shown as it is, never grouped like a count.
        title={t("orders.detail_title", { number: String(order.number) })}
        description={t.rich("orders.detail_placed", {
          time: <DateTime key="time" value={order.placed_at} />,
          channel: channelName.get(order.channel) ?? order.channel,
        })}
        back={back(t)}
        badges={
          <HStack gap={1}>
            {cancelled && <CancelledBadge t={t} />}
            {order.address_review && <NeedsReviewBadge t={t} />}
            <PaymentBadge status={order.financial_status} t={t} />
            <FulfillmentBadge status={order.fulfillment_status} t={t} />
          </HStack>
        }
        actions={
          <>
            {!cancelled && <CancelOrderButton order={order} />}
            {canMarkPaid(order) && (
              <MarkPaid
                id={order.id}
                number={order.number}
                hasPayment={order.payments.some((p) => p.status === "pending")}
              />
            )}
            {canFulfil && <FulfilButton order={order} places={places} />}
          </>
        }
      />

      {cancelled && (
        <Banner
          status="warning"
          title={t("orders.detail_cancelled_title")}
          description={
            order.cancel_reason
              ? t("orders.detail_cancelled_reason", {
                  reason: cancelReasonLabel(t, order.cancel_reason),
                })
              : t("orders.detail_cancelled_no_reason")
          }
        />
      )}

      <div className="detail-columns">
        <div>
          <Card>
            <VStack gap={3}>
              <Text type="display-3" as="h2">
                {t("orders.detail_items")}
              </Text>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>{t("common.product")}</TableHeaderCell>
                    <TableHeaderCell>{t("common.price")}</TableHeaderCell>
                    <TableHeaderCell>{t("common.quantity")}</TableHeaderCell>
                    <TableHeaderCell>{t("orders.detail_column_shipped")}</TableHeaderCell>
                    <TableHeaderCell>{t("common.tax")}</TableHeaderCell>
                    <TableHeaderCell>{t("common.total")}</TableHeaderCell>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {order.lines.map((line) => (
                    <TableRow key={line.id}>
                      <TableCell>
                        <VStack gap={0.5}>
                          <Link href={`/products/${line.product}`}>
                            <Text type="body" weight="medium">
                              {line.product_title}
                            </Text>
                          </Link>
                          {(line.variant_title || line.sku) && (
                            <Text type="supporting" color="secondary">
                              {[
                                line.variant_title,
                                line.sku && t("common.sku_named", { sku: line.sku }),
                              ]
                                .filter(Boolean)
                                .join(" · ")}
                            </Text>
                          )}
                          {line.localized && (
                            <Text type="supporting" color="secondary">
                              {t("orders.detail_line_localized", {
                                title: [line.localized.product_title, line.localized.variant_title]
                                  .filter(Boolean)
                                  .join(" · "),
                              })}
                            </Text>
                          )}
                        </VStack>
                      </TableCell>
                      <TableCell>
                        <UnitPrice line={line} t={t} format={format} listNames={listNames} />
                      </TableCell>
                      <TableCell>{format.number(line.quantity)}</TableCell>
                      <TableCell>
                        {t("orders.detail_line_shipped", {
                          fulfilled: line.fulfilled_quantity,
                          quantity: line.quantity,
                        })}
                      </TableCell>
                      <TableCell>{taxCell(t, format, line.tax)}</TableCell>
                      <TableCell>
                        <Charged
                          before={line.subtotal}
                          total={line.total}
                          shares={line.discounts}
                          discounts={order.discounts}
                          t={t}
                          format={format}
                        />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
                <TableFooter>
                  <TotalRow label={t("common.subtotal")}>{format.money(order.subtotal)}</TotalRow>
                  {order.shipping_lines.map((shipping) => (
                    <TableRow key={shipping.group}>
                      <TableCell>
                        <VStack gap={0.5}>
                          <Text>{t("orders.detail_shipping_line", { name: shipping.name })}</Text>
                          {shipping.localized && (
                            <Text type="supporting" color="secondary">
                              {t("orders.detail_shipping_localized", {
                                name: shipping.localized.name,
                              })}
                            </Text>
                          )}
                          {shipping.provider && (
                            <Text type="supporting" color="secondary">
                              {[shipping.provider, shipping.service, shipping.reference]
                                .filter(Boolean)
                                .join(" · ")}
                            </Text>
                          )}
                        </VStack>
                      </TableCell>
                      <TableCell />
                      <TableCell />
                      <TableCell />
                      <TableCell>{taxCell(t, format, shipping.tax)}</TableCell>
                      <TableCell>
                        <Charged
                          before={shipping.amount}
                          total={shipping.total}
                          shares={shipping.discounts}
                          discounts={order.discounts}
                          t={t}
                          format={format}
                        />
                      </TableCell>
                    </TableRow>
                  ))}
                  <TotalRow
                    label={order.taxes_included ? t("orders.detail_tax_included") : t("common.tax")}
                  >
                    {format.money(order.tax_total)}
                  </TotalRow>
                  <TotalRow
                    label={
                      <Text type="body" weight="bold">
                        {t("common.total")}
                      </Text>
                    }
                  >
                    <Text type="body" weight="bold">
                      {format.money(order.total)}
                    </Text>
                  </TotalRow>
                </TableFooter>
              </Table>
              <DiscountsList order={order} t={t} format={format} />
            </VStack>
          </Card>

          <PaymentCard
            order={order}
            buyerLanguage={buyerLanguage}
            format={format}
            extensions={extensions}
            t={t}
          />

          {fulfillments.length > 0 && (
            <ShipmentsCard order={order} fulfillments={fulfillments} places={places} />
          )}
          {fulfillments.map((f) => (
            <AppSlot
              key={f.id}
              extensions={extensions}
              target="order.fulfillment"
              context={{ order_id: order.id, fulfillment_id: f.id }}
            />
          ))}

          {emails.data && (
            <EmailsCard
              orderId={order.id}
              messages={emails.data.items}
              canResend={me.scopes.includes("notifications:write")}
            />
          )}

          <TimelineCard
            title={t("orders.timeline_title")}
            events={[
              {
                at: order.placed_at,
                what: channelName.has(order.channel)
                  ? t("orders.timeline_placed", { channel: channelName.get(order.channel) })
                  : t("orders.timeline_placed_unknown"),
              },
              ...fulfillments.map((f) => {
                const count = f.lines.reduce((n, l) => n + l.quantity, 0);
                return {
                  at: f.created_at,
                  what: f.cancelled_at
                    ? t("orders.timeline_fulfilled_cancelled", { count })
                    : t("orders.timeline_fulfilled", { count }),
                };
              }),
              ...order.payments.flatMap((p) => [
                ...(p.paid_at
                  ? [
                      {
                        at: p.paid_at,
                        what: p.method_name
                          ? t("orders.timeline_paid_by", {
                              amount: format.money(p.amount),
                              method: p.method_name,
                            })
                          : t("orders.timeline_paid", { amount: format.money(p.amount) }),
                      },
                    ]
                  : []),
                ...(p.voided_at
                  ? [
                      {
                        at: p.voided_at,
                        what: p.method_name
                          ? t("orders.timeline_voided_method", { method: p.method_name })
                          : t("orders.timeline_voided"),
                      },
                    ]
                  : []),
              ]),
              ...(order.cancelled_at
                ? [{ at: order.cancelled_at, what: t("orders.timeline_cancelled") }]
                : []),
            ]}
          />
        </div>
        <div>
          <Card>
            <MetadataList title={t("common.summary")}>
              <MetadataListItem label={t("common.total")}>
                <Text weight="bold">{format.money(order.total)}</Text>
              </MetadataListItem>
              <MetadataListItem label={t("orders.payment")}>
                <PaymentBadge status={order.financial_status} t={t} />
              </MetadataListItem>
              <MetadataListItem label={t("orders.fulfilment")}>
                <FulfillmentBadge status={order.fulfillment_status} t={t} />
              </MetadataListItem>
              <MetadataListItem label={t("orders.detail_summary_items")}>
                {format.number(order.lines.reduce((n, l) => n + l.quantity, 0))}
              </MetadataListItem>
            </MetadataList>
          </Card>

          <Card>
            <MetadataList title={t("common.customer")}>
              <MetadataListItem label={t("common.email")}>
                <Link href={`/customers/${order.customer}`}>{order.email}</Link>
              </MetadataListItem>
              <MetadataListItem label={t("common.country")}>
                {countryName(order.country, countries, format.locale)}
              </MetadataListItem>
              <MetadataListItem label={t("common.currency")}>{order.currency}</MetadataListItem>
              <MetadataListItem label={t("common.channel")}>
                {channelName.get(order.channel) ?? order.channel}
              </MetadataListItem>
              {/* The language the buyer checked out in; none for an order from before
                  orders kept one. */}
              {order.language && (
                <MetadataListItem label={t("common.language")}>
                  {languageLabel(format, order.language)}
                </MetadataListItem>
              )}
            </MetadataList>
          </Card>

          {order.shipping_address && (
            <Card>
              <MetadataList title={t("orders.detail_ships_to")}>
                <MetadataListItem label={t("common.person_name")}>
                  {[order.shipping_address.first_name, order.shipping_address.last_name]
                    .filter(Boolean)
                    .join(" ")}
                </MetadataListItem>
                <MetadataListItem label={t("common.address")}>
                  {[
                    order.shipping_address.company,
                    order.shipping_address.line1,
                    order.shipping_address.line2,
                    [order.shipping_address.postal_code, order.shipping_address.city]
                      .filter(Boolean)
                      .join(" "),
                    order.shipping_address.province_name ?? order.shipping_address.province,
                    countryName(order.shipping_address.country, countries, format.locale),
                  ]
                    .filter(Boolean)
                    .join(", ")}
                </MetadataListItem>
                {order.shipping_address.phone && (
                  <MetadataListItem label={t("common.phone")}>
                    {order.shipping_address.phone}
                  </MetadataListItem>
                )}
              </MetadataList>
              {canWriteOrders && addressFormat && (
                <EditAddressButton order={order} format={addressFormat} />
              )}
            </Card>
          )}

          <AddressCheckCard
            order={order}
            format={addressFormat}
            canWrite={canWriteOrders}
            names={Object.fromEntries([...people, ...apps])}
          />

          {definitions.length > 0 && fields.data && (
            <CustomFieldsCard
              ownerType="order"
              ownerId={order.id}
              definitions={definitions}
              fields={fields.data}
              scopes={me.scopes}
              currency={order.currency}
            />
          )}
          <AppSlot
            extensions={extensions}
            target="order.details"
            context={{ order_id: order.id }}
          />
        </div>
      </div>
    </>
  );
}

type Order = AdminSchemas["AdminOrderResponse"];

/** How the order is paid for: each payment, newest first. */
function PaymentCard({
  order,
  buyerLanguage,
  format,
  extensions,
  t,
}: {
  order: Order;
  /** The order's language when it isn't the store's default: the buyer read another copy. */
  buyerLanguage: string | null;
  format: Format;
  extensions: Extensions;
  t: T;
}) {
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <HStack gap={2} vAlign="center">
            <Text type="display-3" as="h2">
              {t("orders.payment")}
            </Text>
            <PaymentBadge status={order.financial_status} t={t} />
          </HStack>
        </HStack>
        {order.payments.length === 0 ? (
          <Text color="secondary">{t("orders.detail_no_payment")}</Text>
        ) : (
          <VStack gap={4}>
            {order.payments.map((payment) => (
              <VStack key={payment.id} gap={2}>
                <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
                  <HStack gap={2} vAlign="center">
                    <Text weight="medium">
                      {payment.method_name ?? t("orders.detail_payment_by_staff")}
                    </Text>
                    <PaymentStatusBadge status={payment.status} t={t} />
                  </HStack>
                  <Text weight="medium">{format.money(payment.amount)}</Text>
                </HStack>
                <MetadataList>
                  <MetadataListItem label={t("orders.detail_payment_provider")}>
                    {providerLabel(payment.provider, t)}
                  </MetadataListItem>
                  {payment.reference && (
                    <MetadataListItem label={t("common.reference")}>
                      {payment.reference}
                    </MetadataListItem>
                  )}
                  {payment.paid_at && (
                    <MetadataListItem label={t("orders.detail_payment_paid")}>
                      <DateTime value={payment.paid_at} />
                    </MetadataListItem>
                  )}
                  {payment.voided_at && (
                    <MetadataListItem label={t("orders.detail_payment_voided")}>
                      <DateTime value={payment.voided_at} />
                    </MetadataListItem>
                  )}
                  {payment.refunded.amount > 0 && (
                    <MetadataListItem label={t("orders.detail_payment_refunded")}>
                      {format.money(payment.refunded)}
                    </MetadataListItem>
                  )}
                </MetadataList>
                {/* The admin's payment answers its method's name and instructions in the
                    source language only, not the buyer's copy in the order's language:
                    for an order in another language they're labelled as the source. */}
                {payment.instructions && (
                  <VStack gap={1}>
                    <Text type="label">
                      {buyerLanguage
                        ? t("orders.detail_payment_instructions_source")
                        : t("orders.detail_payment_instructions")}
                    </Text>
                    <Text type="supporting" color="secondary">
                      <span style={{ whiteSpace: "pre-wrap" }}>{payment.instructions}</span>
                    </Text>
                    {buyerLanguage && (
                      <Text type="supporting" color="secondary">
                        {t("orders.detail_payment_instructions_source_note", {
                          language: format.languageNameInSentence(buyerLanguage),
                        })}
                      </Text>
                    )}
                  </VStack>
                )}
                <AppActions
                  extensions={extensions}
                  target="payment"
                  context={{ payment_id: payment.id, order_id: order.id }}
                  paymentStatus={payment.status}
                />
                <AppSlot
                  extensions={extensions}
                  target="order.payment"
                  context={{ order_id: order.id, payment_id: payment.id }}
                />
              </VStack>
            ))}
          </VStack>
        )}
      </VStack>
    </Card>
  );
}

/** What happened to the order, oldest first: what the order and its shipments record. */
function TimelineCard({
  title,
  events,
}: {
  title: string;
  events: { at: string; what: string }[];
}) {
  const sorted = events.toSorted((a, b) => a.at.localeCompare(b.at));
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {title}
        </Text>
        <VStack gap={2}>
          {sorted.map((e) => (
            <HStack key={`${e.at}-${e.what}`} gap={3} vAlign="start">
              <div style={{ minWidth: 150 }}>
                <Text type="supporting" color="secondary">
                  <DateTime value={e.at} short />
                </Text>
              </div>
              <Text>{e.what}</Text>
            </HStack>
          ))}
        </VStack>
      </VStack>
    </Card>
  );
}
