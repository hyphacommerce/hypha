"use client";

// The transfers table: number, where from and to, how far it got, when it's due and how
// many units, filtered by the API.

import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ArrowsRightLeftIcon } from "@heroicons/react/24/outline";

import { CalendarDate } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { TransferStatusBadge } from "@/components/status";
import { type TransferSummary, trNumber } from "@/lib/transfers";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

const STATUSES = ["draft", "in_transit", "received", "cancelled"] as const;

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
  const t = useT<"transfers">();
  const format = useFormat();
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "status",
      label: t("common.status"),
      options: STATUSES.map((value) => ({ value, label: t(`transfers.status_${value}`) })),
    },
  ];
  const views: View[] = [
    { label: t("common.all"), params: {} },
    { label: t("transfers.view_drafts"), params: { status: ["draft"] } },
    { label: t("transfers.status_in_transit"), params: { status: ["in_transit"] } },
    { label: t("transfers.status_received"), params: { status: ["received"] } },
  ];
  const place = (id: string) => locations[id] ?? t("transfers.a_location");
  const columns: TableColumn<TransferSummary>[] = [
    {
      key: "number",
      header: t("transfers.column_transfer"),
      width: pixel(120),
      renderCell: (transfer) => (
        <Link href={`/transfers/${transfer.id}`} weight="semibold" color="primary">
          {trNumber(transfer.number)}
        </Link>
      ),
    },
    {
      key: "route",
      header: t("transfers.column_route"),
      width: proportional(2, { minWidth: 220 }),
      renderCell: (transfer) => (
        <Text>
          {place(transfer.origin)} → {place(transfer.destination)}
        </Text>
      ),
    },
    {
      key: "status",
      header: t("common.status"),
      width: pixel(130),
      renderCell: (transfer) => <TransferStatusBadge status={transfer.status} t={t} />,
    },
    {
      key: "expected",
      header: t("transfers.expected"),
      width: pixel(140),
      renderCell: (transfer) =>
        transfer.expected_at ? (
          <CalendarDate value={transfer.expected_at} short />
        ) : (
          <Text color="secondary">—</Text>
        ),
    },
    {
      key: "units",
      header: t("transfers.units"),
      width: pixel(100),
      align: "end",
      renderCell: (transfer) => <Text>{format.number(transfer.units)}</Text>,
    },
  ];
  return (
    <ResourceTable<TransferSummary>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(transfer) => `/transfers/${transfer.id}`}
      isSelectable={false}
      filters={filters}
      views={views}
      error={page?.error}
      empty={{
        icon: <ArrowsRightLeftIcon width={32} />,
        title: t("transfers.list_empty_title"),
        description: canWrite
          ? t("transfers.list_empty_description_write")
          : t("transfers.list_empty_description_read"),
      }}
      noResults={{ title: t("transfers.list_no_results_title") }}
    />
  );
}
