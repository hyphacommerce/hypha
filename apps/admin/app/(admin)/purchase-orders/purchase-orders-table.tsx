"use client";

// The purchase orders table: number, supplier, where it goes, how far it got, when it's
// due and its total, filtered and sorted by the API.

import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ClipboardDocumentListIcon } from "@heroicons/react/24/outline";

import { CalendarDate } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import { PurchaseOrderStatusBadge, ReceiptStatusBadge } from "@/components/status";
import { type PurchaseOrderSummary, poNumber } from "@/lib/purchasing";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

const STATUSES = ["draft", "ordered", "closed", "cancelled"] as const;
const RECEIPTS = ["none", "partial", "received"] as const;

const NO_LOCATIONS: Record<string, string> = {};

export function PurchaseOrdersTable({
  page,
  locations = NO_LOCATIONS,
  canWrite,
}: {
  /** The page of orders; absent while it loads (the toolbar shows meanwhile). */
  page?: { items: PurchaseOrderSummary[]; next_cursor: string | null; error?: string };
  /** Locations' names by ID. */
  locations?: Record<string, string>;
  canWrite: boolean;
}) {
  const t = useT<"purchasing">();
  const format = useFormat();
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "status",
      label: t("common.status"),
      options: STATUSES.map((value) => ({ value, label: t(`purchasing.status_${value}`) })),
    },
    {
      kind: "select",
      param: "receipt_status",
      label: t("purchasing.received"),
      options: RECEIPTS.map((value) => ({ value, label: t(`purchasing.receipt_${value}`) })),
    },
  ];
  const views: View[] = [
    { label: t("common.all"), params: {} },
    { label: t("purchasing.view_drafts"), params: { status: ["draft"] } },
    { label: t("purchasing.view_open"), params: { status: ["ordered"] } },
    { label: t("purchasing.view_closed"), params: { status: ["closed"] } },
  ];
  const sorts: SortOption[] = [
    { value: "number_desc", label: t("purchasing.sort_newest") },
    { value: "number_asc", label: t("purchasing.sort_oldest") },
    { value: "expected_at", label: t("purchasing.sort_due") },
  ];
  const columns: TableColumn<PurchaseOrderSummary>[] = [
    {
      key: "number",
      header: t("common.order"),
      width: pixel(120),
      renderCell: (po) => (
        <Link href={`/purchase-orders/${po.id}`} weight="semibold" color="primary">
          {poNumber(po.number)}
        </Link>
      ),
    },
    {
      key: "supplier",
      header: t("common.supplier"),
      width: proportional(2, { minWidth: 180 }),
      renderCell: (po) => <Text>{po.supplier_name}</Text>,
    },
    {
      key: "destination",
      header: t("purchasing.goes_to"),
      width: proportional(1, { minWidth: 140 }),
      renderCell: (po) => (
        <Text color={po.destination ? undefined : "secondary"}>
          {po.destination
            ? (locations[po.destination] ?? t("purchasing.a_location"))
            : t("common.not_set")}
        </Text>
      ),
    },
    {
      key: "status",
      header: t("common.status"),
      width: pixel(120),
      renderCell: (po) => <PurchaseOrderStatusBadge status={po.status} t={t} />,
    },
    {
      key: "receipt",
      header: t("purchasing.received"),
      width: pixel(150),
      renderCell: (po) =>
        po.status === "draft" || po.status === "cancelled" ? (
          <Text color="secondary">—</Text>
        ) : (
          <ReceiptStatusBadge status={po.receipt_status} t={t} />
        ),
    },
    {
      key: "expected",
      header: t("purchasing.expected"),
      width: pixel(140),
      renderCell: (po) =>
        po.expected_at ? (
          <CalendarDate value={po.expected_at} short />
        ) : (
          <Text color="secondary">—</Text>
        ),
    },
    {
      key: "total",
      header: t("common.total"),
      width: pixel(130),
      align: "end",
      renderCell: (po) => <Text>{po.total ? format.money(po.total) : "—"}</Text>,
    },
  ];
  return (
    <ResourceTable<PurchaseOrderSummary>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(po) => `/purchase-orders/${po.id}`}
      isSelectable={false}
      filters={filters}
      views={views}
      sortOptions={sorts}
      defaultSort="number_desc"
      error={page?.error}
      empty={{
        icon: <ClipboardDocumentListIcon width={32} />,
        title: t("purchasing.list_empty_title"),
        description: canWrite
          ? t("purchasing.list_empty_description_write")
          : t("purchasing.list_empty_description_read"),
      }}
      noResults={{ title: t("purchasing.list_no_results_title") }}
    />
  );
}
