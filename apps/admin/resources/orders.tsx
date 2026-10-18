"use client";

// The order list's table (design note 0024, §3).

import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ShoppingBagIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useState } from "react";

import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { type Currencies, formatMoney } from "@/lib/format";
import { UNPAID } from "@/lib/payments";

import { type BulkOutcome, BulkOutcomeBanner, OrderBulkActions } from "./bulk";

type Order = AdminSchemas["OrderSummaryResponse"];

const SORT_OPTIONS: SortOption[] = [
  { value: "newest", label: "Newest" },
  { value: "oldest", label: "Oldest" },
  { value: "total_desc", label: "Highest total", unavailable: "pick a currency" },
  { value: "total_asc", label: "Lowest total", unavailable: "pick a currency" },
];

const VIEWS: View[] = [
  { label: "All", params: {} },
  {
    label: "Unfulfilled",
    params: { fulfillment_status: ["unfulfilled", "partially_fulfilled"], cancelled: ["false"] },
  },
  { label: "Unpaid", params: { financial_status: UNPAID, cancelled: ["false"] } },
  { label: "Cancelled", params: { cancelled: ["true"] } },
];

const SORTS: Record<string, SortDef> = {
  placed: { asc: "oldest", desc: "newest" },
  total: { asc: "total_asc", desc: "total_desc" },
};

export function OrdersTable({
  page,
  currencies,
  channels,
  countries,
  orderCurrencies,
}: {
  page: { items: Order[]; next_cursor: string | null; error?: string };
  currencies: Currencies;
  /** Channel names by ID. */
  channels: Record<string, string>;
  /** The countries markets sell to: code and name. */
  countries: { value: string; label: string }[];
  /** The currencies markets sell in. */
  orderCurrencies: string[];
}) {
  const [outcome, setOutcome] = useState<BulkOutcome>();
  const filters: FilterDef[] = [
    {
      kind: "search",
      param: "q",
      label: "Search",
      placeholder: "Search orders by number or email",
    },
    {
      kind: "multi",
      param: "financial_status",
      label: "Payment",
      options: [
        { value: "pending", label: "Pending" },
        { value: "authorized", label: "Authorized" },
        { value: "partially_paid", label: "Partly paid" },
        { value: "paid", label: "Paid" },
        { value: "partially_refunded", label: "Partly refunded" },
        { value: "refunded", label: "Refunded" },
        { value: "voided", label: "Voided" },
      ],
    },
    {
      kind: "multi",
      param: "fulfillment_status",
      label: "Fulfilment",
      options: [
        { value: "unfulfilled", label: "Unfulfilled" },
        { value: "partially_fulfilled", label: "Partially fulfilled" },
        { value: "fulfilled", label: "Fulfilled" },
      ],
    },
    {
      kind: "select",
      param: "cancelled",
      label: "Cancelled",
      options: [
        { value: "false", label: "Not cancelled" },
        { value: "true", label: "Cancelled" },
      ],
    },
    { kind: "dates", param: "placed", label: "Placed" },
    {
      kind: "select",
      param: "channel",
      label: "Channel",
      options: Object.entries(channels).map(([value, label]) => ({ value, label })),
    },
    { kind: "select", param: "country", label: "Country", options: countries },
    {
      kind: "select",
      param: "currency",
      label: "Currency",
      options: orderCurrencies.map((c) => ({ value: c, label: c })),
    },
  ];
  const columns: TableColumn<Order>[] = [
    {
      key: "number",
      header: "Order",
      width: pixel(130),
      renderCell: (order) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/orders/${order.id}`} weight="semibold" color="primary">
            #{order.number}
          </Link>
          {order.cancelled_at ? (
            <CancelledBadge />
          ) : (
            <Text type="supporting" color="secondary">
              {order.item_count} {order.item_count === 1 ? "item" : "items"}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "placed",
      header: "Date",
      width: pixel(150),
      renderCell: (order) => (
        <Text color="secondary">
          <DateTime value={order.placed_at} short />
        </Text>
      ),
    },
    {
      key: "email",
      header: "Customer",
      width: proportional(2, { minWidth: 180 }),
      renderCell: (order) => (
        <VStack gap={0.5}>
          <Text weight="medium">{order.email}</Text>
          <Text type="supporting" color="secondary">
            {[order.country, channels[order.channel] ?? order.channel].join(" · ")}
          </Text>
        </VStack>
      ),
    },
    {
      key: "payment",
      header: "Payment",
      width: pixel(130),
      renderCell: (order) =>
        order.cancelled_at ? (
          <Text color="secondary">—</Text>
        ) : (
          <PaymentBadge status={order.financial_status} />
        ),
    },
    {
      key: "fulfilment",
      header: "Fulfilment",
      width: pixel(150),
      renderCell: (order) =>
        order.cancelled_at ? (
          <Text color="secondary">—</Text>
        ) : (
          <FulfillmentBadge status={order.fulfillment_status} />
        ),
    },
    {
      key: "total",
      header: "Total",
      width: pixel(130),
      align: "end",
      renderCell: (order) => <Text weight="medium">{formatMoney(order.total, currencies)}</Text>,
    },
  ];
  return (
    <VStack gap={3}>
      {outcome && (
        <BulkOutcomeBanner outcome={outcome} noun="order" onClose={() => setOutcome(undefined)} />
      )}
      <ResourceTable<Order>
        renderBulkActions={(selected, clear) => (
          <OrderBulkActions selected={selected} clear={clear} onDone={setOutcome} />
        )}
        rowLabel={(o) => `Order #${o.number}`}
        rows={page.items}
        nextCursor={page.next_cursor}
        columns={columns}
        idKey="id"
        rowHref={(o) => `/orders/${o.id}`}
        filters={filters}
        views={VIEWS}
        sorts={SORTS}
        sortOptions={SORT_OPTIONS}
        defaultSort="newest"
        sortRequires={{ total_asc: "currency", total_desc: "currency" }}
        error={page.error}
        empty={{
          icon: <ShoppingBagIcon width={32} />,
          title: "No orders yet",
          description: "Orders placed through checkout show up here, newest first.",
        }}
        noResults={{
          title: "No orders match these filters",
          description:
            "Search finds an order by its number (1001 or #1001) or by text in its email.",
        }}
      />
    </VStack>
  );
}
