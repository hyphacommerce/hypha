"use client";

// The order list's table.

import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ShoppingBagIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState } from "react";

import { customFieldFilters } from "@/components/custom-fields/filters";
import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import {
  CancelledBadge,
  FulfillmentBadge,
  NeedsReviewBadge,
  PaymentBadge,
} from "@/components/status";
import type { Definition } from "@/lib/custom-fields";
import { UNPAID } from "@/lib/payments";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { type BulkOutcome, BulkOutcomeBanner, OrderBulkActions } from "./bulk";

/** No filterable custom fields. */
const NO_FIELDS: Definition[] = [];
const NO_NAMES: Record<string, string> = {};
const NO_OPTIONS: { value: string; label: string }[] = [];
const NO_CODES: string[] = [];

type Order = AdminSchemas["OrderSummaryResponse"];

/** The payment and fulfilment statuses the list filters by, in the order offered. */
const FINANCIAL = [
  "pending",
  "authorized",
  "partially_paid",
  "paid",
  "partially_refunded",
  "refunded",
  "voided",
] as const;
const FULFILLMENT = ["unfulfilled", "partially_fulfilled", "fulfilled"] as const;
const SORTS: Record<string, SortDef> = {
  placed: { asc: "oldest", desc: "newest" },
  total: { asc: "total_asc", desc: "total_desc" },
};

export function OrdersTable({
  page,
  channels = NO_NAMES,
  countries = NO_OPTIONS,
  orderCurrencies = NO_CODES,
  fieldFilters = NO_FIELDS,
}: {
  /** The page of rows; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Order[]; next_cursor: string | null; error?: string };
  /** Channel names by ID. */
  channels?: Record<string, string>;
  /** The countries markets sell to: code and name. */
  countries?: { value: string; label: string }[];
  /** The currencies markets sell in. */
  orderCurrencies?: string[];
  /** Filterable custom fields, offered as filters. */
  fieldFilters?: Definition[];
}) {
  const t = useT<"orders">();
  const format = useFormat();
  const [outcome, setOutcome] = useState<BulkOutcome>();
  const needsCurrency = t("orders.list_sort_needs_currency");
  const sortOptions: SortOption[] = [
    { value: "newest", label: t("orders.list_sort_newest") },
    { value: "oldest", label: t("orders.list_sort_oldest") },
    { value: "total_desc", label: t("orders.list_sort_total_desc"), unavailable: needsCurrency },
    { value: "total_asc", label: t("orders.list_sort_total_asc"), unavailable: needsCurrency },
  ];
  const views: View[] = [
    { label: t("common.all"), params: {} },
    {
      label: t("orders.list_view_unfulfilled"),
      params: { fulfillment_status: ["unfulfilled", "partially_fulfilled"], cancelled: ["false"] },
    },
    {
      label: t("orders.list_view_unpaid"),
      params: { financial_status: UNPAID, cancelled: ["false"] },
    },
    { label: t("orders.list_view_cancelled"), params: { cancelled: ["true"] } },
    {
      label: t("orders.list_view_needs_review"),
      params: { address: ["needs_review"], cancelled: ["false"] },
    },
  ];
  const filters: FilterDef[] = [
    {
      kind: "search",
      param: "q",
      label: t("common.search"),
      placeholder: t("orders.list_search_placeholder"),
    },
    {
      kind: "multi",
      param: "financial_status",
      label: t("orders.payment"),
      options: FINANCIAL.map((value) => ({
        value,
        label: t(`orders.list_filter_financial_${value}`),
      })),
    },
    {
      kind: "multi",
      param: "fulfillment_status",
      label: t("orders.fulfilment"),
      options: FULFILLMENT.map((value) => ({
        value,
        label: t(`orders.list_filter_fulfillment_${value}`),
      })),
    },
    {
      kind: "select",
      param: "cancelled",
      label: t("orders.list_filter_cancelled"),
      options: [
        { value: "false", label: t("orders.list_filter_cancelled_no") },
        { value: "true", label: t("orders.list_filter_cancelled_yes") },
      ],
    },
    {
      kind: "select",
      param: "address",
      label: t("common.address"),
      options: [{ value: "needs_review", label: t("orders.list_filter_address_needs_review") }],
    },
    { kind: "dates", param: "placed", label: t("orders.list_filter_placed") },
    {
      kind: "select",
      param: "channel",
      label: t("common.channel"),
      options: Object.entries(channels).map(([value, label]) => ({ value, label })),
    },
    { kind: "select", param: "country", label: t("common.country"), options: countries },
    {
      kind: "select",
      param: "currency",
      label: t("common.currency"),
      options: orderCurrencies.map((c) => ({ value: c, label: c })),
    },
    ...customFieldFilters(fieldFilters),
  ];
  const columns: TableColumn<Order>[] = [
    {
      key: "number",
      header: t("common.order"),
      width: pixel(130),
      renderCell: (order) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/orders/${order.id}`} weight="semibold" color="primary">
            #{order.number}
          </Link>
          {order.cancelled_at ? (
            <CancelledBadge t={t} />
          ) : order.address_review ? (
            <NeedsReviewBadge t={t} />
          ) : (
            <Text type="supporting" color="secondary">
              {t("common.items_count", { count: order.item_count })}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "placed",
      header: t("common.date"),
      width: pixel(150),
      renderCell: (order) => (
        <Text color="secondary">
          <DateTime value={order.placed_at} short />
        </Text>
      ),
    },
    {
      key: "email",
      header: t("common.customer"),
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
      header: t("orders.payment"),
      width: pixel(130),
      renderCell: (order) =>
        order.cancelled_at ? (
          <Text color="secondary">{t("common.empty_value")}</Text>
        ) : (
          <PaymentBadge status={order.financial_status} t={t} />
        ),
    },
    {
      key: "fulfilment",
      header: t("orders.fulfilment"),
      width: pixel(150),
      renderCell: (order) =>
        order.cancelled_at ? (
          <Text color="secondary">{t("common.empty_value")}</Text>
        ) : (
          <FulfillmentBadge status={order.fulfillment_status} t={t} />
        ),
    },
    {
      key: "total",
      header: t("common.total"),
      width: pixel(130),
      align: "end",
      renderCell: (order) => <Text weight="medium">{format.money(order.total)}</Text>,
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
        rowLabel={(o) => t("orders.detail_title", { number: String(o.number) })}
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns}
        idKey="id"
        rowHref={(o) => `/orders/${o.id}`}
        filters={filters}
        views={views}
        sorts={SORTS}
        sortOptions={sortOptions}
        defaultSort="newest"
        sortRequires={{ total_asc: "currency", total_desc: "currency" }}
        error={page?.error}
        empty={{
          icon: <ShoppingBagIcon width={32} />,
          title: t("orders.list_empty_title"),
          description: t("orders.list_empty_description"),
        }}
        noResults={{
          title: t("orders.list_no_results_title"),
          description: t("orders.list_no_results_description"),
        }}
      />
    </VStack>
  );
}
