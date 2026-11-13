"use client";

// The purchase orders table: number, supplier, where it goes, how far it got, when it's
// due and its total, filtered and sorted by the API.

import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ClipboardDocumentListIcon } from "@heroicons/react/24/outline";

import { useCurrencies } from "@/components/currencies";
import { CalendarDate } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import { PurchaseOrderStatusBadge, ReceiptStatusBadge } from "@/components/status";
import { formatMoney } from "@/lib/format";
import { type PurchaseOrderSummary, poNumber } from "@/lib/purchasing";
import { useT } from "@/lib/use-t";

const FILTERS: FilterDef[] = [
  {
    kind: "select",
    param: "status",
    label: "Status",
    options: [
      { value: "draft", label: "Draft" },
      { value: "ordered", label: "Ordered" },
      { value: "closed", label: "Closed" },
      { value: "cancelled", label: "Cancelled" },
    ],
  },
  {
    kind: "select",
    param: "receipt_status",
    label: "Received",
    options: [
      { value: "none", label: "Not received" },
      { value: "partial", label: "Partly received" },
      { value: "received", label: "Received" },
    ],
  },
];

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Drafts", params: { status: ["draft"] } },
  { label: "Open", params: { status: ["ordered"] } },
  { label: "Closed", params: { status: ["closed"] } },
];

const SORTS: SortOption[] = [
  { value: "number_desc", label: "Newest first" },
  { value: "number_asc", label: "Oldest first" },
  { value: "expected_at", label: "Due soonest" },
];

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
  const currencies = useCurrencies();
  const t = useT();
  const columns: TableColumn<PurchaseOrderSummary>[] = [
    {
      key: "number",
      header: "Order",
      width: pixel(120),
      renderCell: (po) => (
        <Link href={`/purchase-orders/${po.id}`} weight="semibold" color="primary">
          {poNumber(po.number)}
        </Link>
      ),
    },
    {
      key: "supplier",
      header: "Supplier",
      width: proportional(2, { minWidth: 180 }),
      renderCell: (po) => <Text>{po.supplier_name}</Text>,
    },
    {
      key: "destination",
      header: "Goes to",
      width: proportional(1, { minWidth: 140 }),
      renderCell: (po) => (
        <Text color={po.destination ? undefined : "secondary"}>
          {po.destination ? (locations[po.destination] ?? "A location") : "Not set"}
        </Text>
      ),
    },
    {
      key: "status",
      header: "Status",
      width: pixel(120),
      renderCell: (po) => <PurchaseOrderStatusBadge status={po.status} t={t} />,
    },
    {
      key: "receipt",
      header: "Received",
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
      header: "Expected",
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
      header: "Total",
      width: pixel(130),
      align: "end",
      renderCell: (po) => <Text>{po.total ? formatMoney(po.total, currencies) : "—"}</Text>,
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
      filters={FILTERS}
      views={VIEWS}
      sortOptions={SORTS}
      defaultSort="number_desc"
      error={page?.error}
      empty={{
        icon: <ClipboardDocumentListIcon width={32} />,
        title: "No purchase orders yet",
        description: canWrite
          ? "Order stock from a supplier: lines start from their catalogue's prices."
          : "Orders placed with suppliers show here.",
      }}
      noResults={{ title: "No purchase orders match these filters" }}
    />
  );
}
