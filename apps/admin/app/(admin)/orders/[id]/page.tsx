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

import { PageHeader } from "@/components/page-header";
import { FulfillmentBadge, PaymentBadge } from "@/components/status";
import { formatDateTime, formatMoney } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

import { MarkPaid } from "./mark-paid";

/** "25%" from parts per million (250000). */
function percent(ppm: number): string {
  return `${new Intl.NumberFormat("en-IE", { maximumFractionDigits: 4 }).format(ppm / 10_000)}%`;
}

export default async function OrderPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const client = await sessionClient();
  const order = loaded(await client.GET("/admin/orders/{id}", { params: { path: { id } } }));

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={`Order #${order.number}`}
          description={`Placed ${formatDateTime(order.placed_at)}`}
          back={{ label: "Orders", href: "/orders" }}
          badges={
            <HStack gap={1}>
              <PaymentBadge status={order.financial_status} />
              <FulfillmentBadge status={order.fulfillment_status} />
            </HStack>
          }
          actions={order.financial_status === "pending" && <MarkPaid id={order.id} />}
        />

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
                      {line.tax
                        ? `${formatMoney(line.tax.amount)} (${percent(line.tax.rate_ppm)})`
                        : "—"}
                    </TableCell>
                    <TableCell>{formatMoney(line.total)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
              <TableFooter>
                <TableRow>
                  <TableCell>Subtotal</TableCell>
                  <TableCell />
                  <TableCell />
                  <TableCell />
                  <TableCell>{formatMoney(order.subtotal)}</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>{order.taxes_included ? "Tax (included)" : "Tax"}</TableCell>
                  <TableCell />
                  <TableCell />
                  <TableCell />
                  <TableCell>{formatMoney(order.tax_total)}</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell>
                    <Text type="body" weight="bold">
                      Total
                    </Text>
                  </TableCell>
                  <TableCell />
                  <TableCell />
                  <TableCell />
                  <TableCell>
                    <Text type="body" weight="bold">
                      {formatMoney(order.total)}
                    </Text>
                  </TableCell>
                </TableRow>
              </TableFooter>
            </Table>
          </VStack>
        </Card>

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
