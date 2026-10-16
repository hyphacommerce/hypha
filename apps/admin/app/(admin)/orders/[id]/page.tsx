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
import type { ReactNode } from "react";

import { PageHeader } from "@/components/page-header";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { CANCEL_REASONS, formatDateTime, formatMoney } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

import { CancelOrderButton, FulfilButton, type Place, ShipmentsCard } from "./fulfilment";
import { MarkPaid } from "./mark-paid";

/** "25%" from parts per million (250000). */
function percent(ppm: number): string {
  return `${new Intl.NumberFormat("en-IE", { maximumFractionDigits: 4 }).format(ppm / 10_000)}%`;
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

export default async function OrderPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const order = loaded(await client.GET("/admin/orders/{id}", { params: { path: { id } } }));
  // Shipments and locations need their own scopes; without them the page shows the
  // order alone.
  const [shipments, locations] = await Promise.all([
    client.GET("/admin/orders/{id}/fulfillments", { params: { path: { id } } }),
    client.GET("/admin/locations"),
  ]);
  const fulfillments = shipments.data?.items ?? [];
  const places: Place[] = (locations.data?.items ?? []).map(({ id, name }) => ({ id, name }));
  const cancelled = order.cancelled_at !== null && order.cancelled_at !== undefined;
  const canFulfil =
    !cancelled && order.unfulfilled.length > 0 && places.length > 0 && shipments.data;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={`Order #${order.number}`}
          description={`Placed ${formatDateTime(order.placed_at)}`}
          back={{ label: "Orders", href: "/orders" }}
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
              {!cancelled && order.financial_status === "pending" && <MarkPaid id={order.id} />}
              {canFulfil && <FulfilButton order={order} places={places} />}
            </>
          }
        />

        {cancelled && (
          <Banner
            status="warning"
            title={`Cancelled ${formatDateTime(order.cancelled_at ?? "")}`}
            description={`${CANCEL_REASONS[order.cancel_reason ?? ""] ?? order.cancel_reason ?? "No reason given"}. Its stock was released.`}
          />
        )}

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
                        <Text type="body" weight="medium">
                          {line.product_title}
                        </Text>
                        {(line.variant_title || line.sku) && (
                          <Text type="supporting" color="secondary">
                            {[line.variant_title, line.sku && `SKU ${line.sku}`]
                              .filter(Boolean)
                              .join(" · ")}
                          </Text>
                        )}
                      </VStack>
                    </TableCell>
                    <TableCell>{formatMoney(line.unit_price)}</TableCell>
                    <TableCell>{line.quantity}</TableCell>
                    <TableCell>
                      {line.fulfilled_quantity} of {line.quantity}
                    </TableCell>
                    <TableCell>
                      {line.tax
                        ? `${formatMoney(line.tax.amount)} (${percent(line.tax.rate_ppm)})`
                        : "—"}
                    </TableCell>
                    <TableCell>{formatMoney(line.total)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
              <TableFooter>
                <TotalRow label="Subtotal">{formatMoney(order.subtotal)}</TotalRow>
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
                        ? `${formatMoney(shipping.tax.amount)} (${percent(shipping.tax.rate_ppm)})`
                        : "—"}
                    </TableCell>
                    <TableCell>{formatMoney(shipping.amount)}</TableCell>
                  </TableRow>
                ))}
                <TotalRow label={order.taxes_included ? "Tax (included)" : "Tax"}>
                  {formatMoney(order.tax_total)}
                </TotalRow>
                <TotalRow
                  label={
                    <Text type="body" weight="bold">
                      Total
                    </Text>
                  }
                >
                  <Text type="body" weight="bold">
                    {formatMoney(order.total)}
                  </Text>
                </TotalRow>
              </TableFooter>
            </Table>
          </VStack>
        </Card>

        {fulfillments.length > 0 && (
          <ShipmentsCard order={order} fulfillments={fulfillments} places={places} />
        )}

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
                  order.shipping_address.country,
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

        <Card>
          <MetadataList title="Customer">
            <MetadataListItem label="Email">{order.email}</MetadataListItem>
            <MetadataListItem label="Country">{order.country}</MetadataListItem>
            <MetadataListItem label="Currency">{order.currency}</MetadataListItem>
          </MetadataList>
        </Card>
      </VStack>
    </Section>
  );
}
