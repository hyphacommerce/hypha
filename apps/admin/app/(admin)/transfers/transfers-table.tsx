"use client";

// The transfers table: number, where from and to, how far it got, when it's due and how
// many units, filtered by the API.

import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ArrowsRightLeftIcon } from "@heroicons/react/24/outline";

import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { TransferStatusBadge } from "@/components/status";
import { type TransferSummary, trNumber } from "@/lib/transfers";

const FILTERS: FilterDef[] = [
  {
    kind: "select",
    param: "status",
    label: "Status",
    options: [
      { value: "draft", label: "Draft" },
      { value: "in_transit", label: "In transit" },
      { value: "received", label: "Received" },
      { value: "cancelled", label: "Cancelled" },
    ],
  },
];

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Drafts", params: { status: ["draft"] } },
  { label: "In transit", params: { status: ["in_transit"] } },
  { label: "Received", params: { status: ["received"] } },
];

const NO_LOCATIONS: Record<string, string> = {};

export function TransfersTable({
  page,
  locations = NO_LOCATIONS,
  canWrite,
}: {
  /** The page of transfers; absent while it loads (the toolbar shows meanwhile). */
  page?: { items: TransferSummary[]; next_cursor: string | null; error?: string };
  /** Locations' names by ID. */
  locations?: Record<string, string>;
  canWrite: boolean;
}) {
  const place = (id: string) => locations[id] ?? "A location";
  const columns: TableColumn<TransferSummary>[] = [
    {
      key: "number",
      header: "Transfer",
      width: pixel(120),
      renderCell: (t) => (
        <Link href={`/transfers/${t.id}`} weight="semibold" color="primary">
          {trNumber(t.number)}
        </Link>
      ),
    },
    {
      key: "route",
      header: "From → to",
      width: proportional(2, { minWidth: 220 }),
      renderCell: (t) => (
        <Text>
          {place(t.origin)} → {place(t.destination)}
        </Text>
      ),
    },
    {
      key: "status",
      header: "Status",
      width: pixel(130),
      renderCell: (t) => <TransferStatusBadge status={t.status} />,
    },
    {
      key: "expected",
      header: "Expected",
      width: pixel(140),
      renderCell: (t) =>
        t.expected_at ? <DateTime value={t.expected_at} short /> : <Text color="secondary">—</Text>,
    },
    {
      key: "units",
      header: "Units",
      width: pixel(100),
      align: "end",
      renderCell: (t) => <Text>{t.units}</Text>,
    },
  ];
  return (
    <ResourceTable<TransferSummary>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(t) => `/transfers/${t.id}`}
      isSelectable={false}
      filters={FILTERS}
      views={VIEWS}
      error={page?.error}
      empty={{
        icon: <ArrowsRightLeftIcon width={32} />,
        title: "No transfers yet",
        description: canWrite
          ? "Move stock between your locations: it leaves one when shipped and reaches the other when received."
          : "Stock moved between locations shows here.",
      }}
      noResults={{ title: "No transfers match these filters" }}
    />
  );
}
