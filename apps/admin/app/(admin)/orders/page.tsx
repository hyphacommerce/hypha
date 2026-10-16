import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Section } from "@astryxdesign/core/Section";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { ShoppingBagIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";

import { PageHeader } from "@/components/page-header";
import { Pager } from "@/components/pager";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { formatDateTime, formatMoney } from "@/lib/format";
import { channelNames, loaded, sessionClient } from "@/lib/hypha";

export default async function OrdersPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const cursor = typeof params.cursor === "string" ? params.cursor : undefined;

  const client = await sessionClient();
  const result = await client.GET("/admin/orders", { params: { query: { cursor } } });
  // A hand-edited URL (an old or foreign cursor): start the list again.
  if (result.response.status === 400) redirect("/orders");
  const page = loaded(result);
  const channelName = await channelNames(client);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader title="Orders" />

        {page.items.length === 0 ? (
          <EmptyState
            icon={<ShoppingBagIcon width={32} />}
            title={cursor ? "No more orders" : "No orders yet"}
            description="Orders placed through checkout show up here, newest first."
          />
        ) : (
          <Table hasHover>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Order</TableHeaderCell>
                <TableHeaderCell>Placed</TableHeaderCell>
                <TableHeaderCell>Customer</TableHeaderCell>
                <TableHeaderCell>Channel</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>Items</TableHeaderCell>
                <TableHeaderCell>Total</TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.items.map((order) => (
                <TableRow key={order.id}>
                  <TableCell>
                    <Link href={`/orders/${order.id}`} weight="medium">
                      #{order.number}
                    </Link>
                  </TableCell>
                  <TableCell>{formatDateTime(order.placed_at)}</TableCell>
                  <TableCell>{order.email}</TableCell>
                  <TableCell>{channelName.get(order.channel) ?? order.channel}</TableCell>
                  <TableCell>
                    <HStack gap={1} wrap="wrap">
                      {order.cancelled_at && <CancelledBadge />}
                      <PaymentBadge status={order.financial_status} />
                      <FulfillmentBadge status={order.fulfillment_status} />
                    </HStack>
                  </TableCell>
                  <TableCell>{order.item_count}</TableCell>
                  <TableCell>{formatMoney(order.total)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}

        <Pager path="/orders" query={{}} isFirstPage={!cursor} nextCursor={page.next_cursor} />
      </VStack>
    </Section>
  );
}
