"use client";

// The orders a discount was used on, newest first, with what it took off each and in
// all.

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { ShoppingBagIcon } from "@heroicons/react/24/outline";

import { useCurrencies } from "@/components/currencies";
import { DateTime } from "@/components/date-time";
import { ResourceTable } from "@/components/resource-table/resource-table";
import { formatMoney } from "@/lib/format";

type Usage = AdminSchemas["DiscountUsageResponse"];
type Money = AdminSchemas["Money"];

export function UsageTable({
  page,
  used,
  totals,
}: {
  /** The page of usage records; absent while it loads. */
  page?: { items: Usage[]; next_cursor: string | null; error?: string };
  /** Orders it was used on (the discount's count). */
  used?: number;
  /** What it took off in all, by currency; `partial` when only the latest were added. */
  totals?: { totals: Money[]; partial: boolean };
}) {
  const currencies = useCurrencies();
  const columns: TableColumn<Usage>[] = [
    {
      key: "order",
      header: "Order",
      width: proportional(2, { minWidth: 200 }),
      renderCell: (u) => (
        <Link href={`/orders/${u.order}`} weight="semibold" color="primary">
          {u.order}
        </Link>
      ),
    },
    {
      key: "customer",
      header: "Customer",
      width: proportional(2, { minWidth: 200 }),
      renderCell: (u) => <Link href={`/customers/${u.customer}`}>{u.customer}</Link>,
    },
    {
      key: "code",
      header: "Code",
      width: pixel(160),
      renderCell: (u) => (
        <Text color={u.code ? undefined : "secondary"}>{u.code ?? "Automatic"}</Text>
      ),
    },
    {
      key: "amount",
      header: "Took off",
      width: pixel(130),
      align: "end",
      renderCell: (u) => <Text>{formatMoney(u.amount, currencies)}</Text>,
    },
    {
      key: "used_at",
      header: "Used",
      width: pixel(170),
      renderCell: (u) => (
        <Text type="supporting">
          <DateTime value={u.used_at} short />
        </Text>
      ),
    },
  ];
  return (
    <VStack gap={4}>
      {used !== undefined && totals && (
        <Card>
          <MetadataList>
            <MetadataListItem label="Orders">
              <Text>{used.toLocaleString("en")}</Text>
            </MetadataListItem>
            <MetadataListItem label="Total discounted">
              <Text>
                {totals.totals.length === 0
                  ? "Nothing yet"
                  : totals.totals.map((m) => formatMoney(m, currencies)).join(" + ")}
                {totals.partial && " (latest 2,000 orders)"}
              </Text>
            </MetadataListItem>
          </MetadataList>
        </Card>
      )}
      <ResourceTable<Usage>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns}
        idKey="order"
        rowHref={(u) => `/orders/${u.order}`}
        isSelectable={false}
        error={page?.error}
        empty={{
          icon: <ShoppingBagIcon width={32} />,
          title: "Not used yet",
          description: "Orders placed with this discount show here.",
        }}
      />
    </VStack>
  );
}
