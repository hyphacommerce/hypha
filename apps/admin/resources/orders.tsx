"use client";

// The order list's table (design note 0024, §3).

import { HStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { ShoppingBagIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";

import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
} from "@/components/resource-table/resource-table";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { type Currencies, formatDateTime, formatMoney } from "@/lib/format";

type Order = AdminSchemas["OrderSummaryResponse"];

const SORT_OPTIONS: SortOption[] = [
  { value: "newest", label: "Newest" },
  { value: "oldest", label: "Oldest" },
  { value: "total_desc", label: "Highest total", unavailable: "pick a currency" },
  { value: "total_asc", label: "Lowest total", unavailable: "pick a currency" },
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
  const filters: FilterDef[] = [
    { kind: "search", param: "q", label: "Search", placeholder: "Order number or email" },
    {
      kind: "multi",
      param: "financial_status",
      label: "Payment",
      options: [
        { value: "pending", label: "Pending" },
        { value: "paid", label: "Paid" },
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
      width: pixel(110),
      renderCell: (order) => (
        <Link href={`/orders/${order.id}`} weight="medium">
          #{order.number}
        </Link>
      ),
    },
    {
      key: "placed",
      header: "Placed",
      width: pixel(190),
      renderCell: (order) => formatDateTime(order.placed_at),
    },
    { key: "email", header: "Customer" },
    {
      key: "channel",
      header: "Channel",
      renderCell: (order) => channels[order.channel] ?? order.channel,
    },
    {
      key: "status",
      header: "Status",
      renderCell: (order) => (
        <HStack gap={1} wrap="wrap">
          {order.cancelled_at && <CancelledBadge />}
          <PaymentBadge status={order.financial_status} />
          <FulfillmentBadge status={order.fulfillment_status} />
        </HStack>
      ),
    },
    { key: "item_count", header: "Items", width: pixel(80), align: "end" },
    {
      key: "total",
      header: "Total",
      width: pixel(130),
      align: "end",
      renderCell: (order) => formatMoney(order.total, currencies),
    },
  ];
  return (
    <ResourceTable<Order>
      rows={page.items}
      nextCursor={page.next_cursor}
      columns={columns}
      idKey="id"
      rowHref={(o) => `/orders/${o.id}`}
      filters={filters}
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
        description: "Search finds an order by its number (1001 or #1001) or by text in its email.",
      }}
    />
  );
}
