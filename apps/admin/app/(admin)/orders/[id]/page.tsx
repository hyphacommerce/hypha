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
import type { AdminSchemas } from "@hypha/sdk/admin";
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
  PaymentBadge,
  PaymentStatusBadge,
} from "@/components/status";
import { CANCEL_REASONS, type Currencies, formatMoney } from "@/lib/format";
import {
  channelNames,
  loadCountries,
  loadCurrencies,
  loaded,
  requireStaff,
  sessionClient,
} from "@/lib/hypha";
import { type Extensions, loadExtensions } from "@/lib/app-extensions";
import { definitionsOf } from "@/lib/custom-fields-server";
import { canMarkPaid, providerLabel } from "@/lib/payments";

import { EmailsCard } from "./emails";
import { CancelOrderButton, FulfilButton, type Place, ShipmentsCard } from "./fulfilment";
import { MarkPaid } from "./mark-paid";

const RATE = new Intl.NumberFormat("en-IE", { maximumFractionDigits: 4 });

const BACK: Crumb = { label: "Orders", href: "/orders" };

/** "25%" from parts per million (250000). */
function percent(ppm: number): string {
  return `${RATE.format(ppm / 10_000)}%`;
}

/** "1 item", "3 items". */
function items(n: number): string {
  return `${n} ${n === 1 ? "item" : "items"}`;
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
  const { id } = await params;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <Order id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Order({ id }: { id: string }) {
  const client = await sessionClient();
  // All at once; a missing order still goes to the 404 page (`loaded`). Shipments and
  // locations need their own scopes; without them the page shows the order alone. The
  // emails need `notifications:read` and `orders:read` (they name the buyer); without
  // them the request is refused and the card isn't shown. The staff member is the
  // layout's read too. What installed apps add here is one more read.
  const [
    found,
    shipments,
    locations,
    channelName,
    currencies,
    countries,
    emails,
    me,
    extensions,
    definitions,
    fields,
  ] = await Promise.all([
    client.GET("/admin/orders/{id}", { params: { path: { id } } }),
    client.GET("/admin/orders/{id}/fulfillments", { params: { path: { id } } }),
    client.GET("/admin/locations"),
    channelNames(client),
    loadCurrencies(),
    loadCountries(),
    client.GET("/admin/notifications", { params: { query: { order: id } } }),
    requireStaff(),
    loadExtensions(),
    // Its custom fields: none shown without `custom_fields:read`.
    definitionsOf("order"),
    client.GET("/admin/orders/{id}/custom-fields", { params: { path: { id } } }),
  ]);
  const order = loaded(found);
  const countryName = (code: string) => countries.find((c) => c.code === code)?.name ?? code;
  const fulfillments = shipments.data?.items ?? [];
  const places: Place[] = (locations.data?.items ?? []).map((l) => ({ id: l.id, name: l.name }));
  const cancelled = order.cancelled_at !== null && order.cancelled_at !== undefined;
  const canFulfil =
    !cancelled && order.unfulfilled.length > 0 && places.length > 0 && shipments.data;

  return (
    <>
      <PageHeader
        title={`Order #${order.number}`}
        description={
          <>
            Placed <DateTime value={order.placed_at} /> ·{" "}
            {channelName.get(order.channel) ?? order.channel}
          </>
        }
        back={BACK}
        badges={
          <HStack gap={1}>
            {cancelled && <CancelledBadge />}
            <PaymentBadge status={order.financial_status} />
            <FulfillmentBadge status={order.fulfillment_status} />
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
          title="Cancelled"
          description={`${CANCEL_REASONS[order.cancel_reason ?? ""] ?? order.cancel_reason ?? "No reason given"}. Its stock was released.`}
        />
      )}

      <div className="detail-columns">
        <div>
          <Card>
            <VStack gap={3}>
              <Text type="display-3" as="h2">
                Items
              </Text>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>Product</TableHeaderCell>
                    <TableHeaderCell>Price</TableHeaderCell>
                    <TableHeaderCell>Quantity</TableHeaderCell>
                    <TableHeaderCell>Shipped</TableHeaderCell>
                    <TableHeaderCell>Tax</TableHeaderCell>
                    <TableHeaderCell>Total</TableHeaderCell>
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
                              {[line.variant_title, line.sku && `SKU ${line.sku}`]
                                .filter(Boolean)
                                .join(" · ")}
                            </Text>
                          )}
                        </VStack>
                      </TableCell>
                      <TableCell>{formatMoney(line.unit_price, currencies)}</TableCell>
                      <TableCell>{line.quantity}</TableCell>
                      <TableCell>
                        {line.fulfilled_quantity} of {line.quantity}
                      </TableCell>
                      <TableCell>
                        {line.tax
                          ? `${formatMoney(line.tax.amount, currencies)} (${percent(line.tax.rate_ppm)})`
                          : "—"}
                      </TableCell>
                      <TableCell>{formatMoney(line.total, currencies)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
                <TableFooter>
                  <TotalRow label="Subtotal">{formatMoney(order.subtotal, currencies)}</TotalRow>
                  {order.shipping_lines.map((shipping) => (
                    <TableRow key={shipping.group}>
                      <TableCell>
                        <VStack gap={0.5}>
                          <Text>Shipping: {shipping.name}</Text>
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
                      <TableCell>
                        {shipping.tax
                          ? `${formatMoney(shipping.tax.amount, currencies)} (${percent(shipping.tax.rate_ppm)})`
                          : "—"}
                      </TableCell>
                      <TableCell>{formatMoney(shipping.amount, currencies)}</TableCell>
                    </TableRow>
                  ))}
                  <TotalRow label={order.taxes_included ? "Tax (included)" : "Tax"}>
                    {formatMoney(order.tax_total, currencies)}
                  </TotalRow>
                  <TotalRow
                    label={
                      <Text type="body" weight="bold">
                        Total
                      </Text>
                    }
                  >
                    <Text type="body" weight="bold">
                      {formatMoney(order.total, currencies)}
                    </Text>
                  </TotalRow>
                </TableFooter>
              </Table>
            </VStack>
          </Card>

          <PaymentCard order={order} currencies={currencies} extensions={extensions} />

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
            events={[
              {
                at: order.placed_at,
                what: `Placed through ${channelName.get(order.channel) ?? "a channel"}`,
              },
              ...fulfillments.map((f) => ({
                at: f.created_at,
                what: `Fulfilled ${items(f.lines.reduce((n, l) => n + l.quantity, 0))}${f.cancelled_at ? " (cancelled since)" : ""}`,
              })),
              ...order.payments.flatMap((p) => [
                ...(p.paid_at
                  ? [
                      {
                        at: p.paid_at,
                        what: `Paid ${formatMoney(p.amount, currencies)}${p.method_name ? ` by ${p.method_name}` : ""}`,
                      },
                    ]
                  : []),
                ...(p.voided_at
                  ? [
                      {
                        at: p.voided_at,
                        what: `Payment voided${p.method_name ? ` (${p.method_name})` : ""}`,
                      },
                    ]
                  : []),
              ]),
              ...(order.cancelled_at ? [{ at: order.cancelled_at, what: "Cancelled" }] : []),
            ]}
          />
        </div>
        <div>
          <Card>
            <MetadataList title="Summary">
              <MetadataListItem label="Total">
                <Text weight="bold">{formatMoney(order.total, currencies)}</Text>
              </MetadataListItem>
              <MetadataListItem label="Payment">
                <PaymentBadge status={order.financial_status} />
              </MetadataListItem>
              <MetadataListItem label="Fulfilment">
                <FulfillmentBadge status={order.fulfillment_status} />
              </MetadataListItem>
              <MetadataListItem label="Items">
                {order.lines.reduce((n, l) => n + l.quantity, 0)}
              </MetadataListItem>
            </MetadataList>
          </Card>

          <Card>
            <MetadataList title="Customer">
              <MetadataListItem label="Email">
                <Link href={`/customers/${order.customer}`}>{order.email}</Link>
              </MetadataListItem>
              <MetadataListItem label="Country">{countryName(order.country)}</MetadataListItem>
              <MetadataListItem label="Currency">{order.currency}</MetadataListItem>
              <MetadataListItem label="Channel">
                {channelName.get(order.channel) ?? order.channel}
              </MetadataListItem>
            </MetadataList>
          </Card>

          {order.shipping_address && (
            <Card>
              <MetadataList title="Ships to">
                <MetadataListItem label="Name">
                  {[order.shipping_address.first_name, order.shipping_address.last_name]
                    .filter(Boolean)
                    .join(" ")}
                </MetadataListItem>
                <MetadataListItem label="Address">
                  {[
                    order.shipping_address.company,
                    order.shipping_address.line1,
                    order.shipping_address.line2,
                    [order.shipping_address.postal_code, order.shipping_address.city]
                      .filter(Boolean)
                      .join(" "),
                    order.shipping_address.province,
                    countryName(order.shipping_address.country),
                  ]
                    .filter(Boolean)
                    .join(", ")}
                </MetadataListItem>
                {order.shipping_address.phone && (
                  <MetadataListItem label="Phone">{order.shipping_address.phone}</MetadataListItem>
                )}
              </MetadataList>
            </Card>
          )}

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
  currencies,
  extensions,
}: {
  order: Order;
  currencies: Currencies;
  extensions: Extensions;
}) {
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <HStack gap={2} vAlign="center">
            <Text type="display-3" as="h2">
              Payment
            </Text>
            <PaymentBadge status={order.financial_status} />
          </HStack>
        </HStack>
        {order.payments.length === 0 ? (
          <Text color="secondary">
            No payment method. Mark the order paid when you collect the money, and a manual payment
            of the total is recorded.
          </Text>
        ) : (
          <VStack gap={4}>
            {order.payments.map((payment) => (
              <VStack key={payment.id} gap={2}>
                <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
                  <HStack gap={2} vAlign="center">
                    <Text weight="medium">{payment.method_name ?? "Recorded by staff"}</Text>
                    <PaymentStatusBadge status={payment.status} />
                  </HStack>
                  <Text weight="medium">{formatMoney(payment.amount, currencies)}</Text>
                </HStack>
                <MetadataList>
                  <MetadataListItem label="Provider">
                    {providerLabel(payment.provider)}
                  </MetadataListItem>
                  {payment.reference && (
                    <MetadataListItem label="Reference">{payment.reference}</MetadataListItem>
                  )}
                  {payment.paid_at && (
                    <MetadataListItem label="Paid">
                      <DateTime value={payment.paid_at} />
                    </MetadataListItem>
                  )}
                  {payment.voided_at && (
                    <MetadataListItem label="Voided">
                      <DateTime value={payment.voided_at} />
                    </MetadataListItem>
                  )}
                  {payment.refunded.amount > 0 && (
                    <MetadataListItem label="Refunded">
                      {formatMoney(payment.refunded, currencies)}
                    </MetadataListItem>
                  )}
                </MetadataList>
                {payment.instructions && (
                  <VStack gap={1}>
                    <Text type="label">Instructions the shopper saw</Text>
                    <Text type="supporting" color="secondary">
                      <span style={{ whiteSpace: "pre-wrap" }}>{payment.instructions}</span>
                    </Text>
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
function TimelineCard({ events }: { events: { at: string; what: string }[] }) {
  const sorted = events.toSorted((a, b) => a.at.localeCompare(b.at));
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          Timeline
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
