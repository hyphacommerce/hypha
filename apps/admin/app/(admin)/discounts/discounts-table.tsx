"use client";

// The discounts table: title, how buyers get it, what it gives, where it stands, its
// window and its uses, filtered and searched by the API.

import { Button } from "@astryxdesign/core/Button";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ReceiptPercentIcon } from "@heroicons/react/24/outline";

import { DiscountStatusBadge } from "@/components/discounts/status-badge";
import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { type Discount, methodLabel, usesLabel } from "@/lib/discounts/wire";
import { PHASES } from "@/lib/price-lists";

import { ListWindow } from "../markets/price-lists/price-lists-table";

const FILTERS: FilterDef[] = [
  { kind: "search", param: "q", label: "Search", placeholder: "Search titles and codes" },
  {
    kind: "select",
    param: "status",
    label: "Status",
    options: PHASES.map((p) => ({ value: p.value, label: p.label })),
  },
  {
    kind: "select",
    param: "method",
    label: "Method",
    options: [
      { value: "codes", label: "Codes" },
      { value: "automatic", label: "Automatic" },
    ],
  },
  {
    kind: "select",
    param: "class",
    label: "Takes off",
    options: [
      { value: "product", label: "Products" },
      { value: "order", label: "The order" },
      { value: "shipping", label: "Shipping" },
    ],
  },
];

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Active", params: { status: ["active"] } },
  { label: "Scheduled", params: { status: ["scheduled"] } },
  { label: "Codes", params: { method: ["codes"] } },
  { label: "Automatic", params: { method: ["automatic"] } },
];

const COLUMNS: TableColumn<Discount>[] = [
  {
    key: "title",
    header: "Discount",
    width: proportional(2, { minWidth: 200 }),
    renderCell: (d) => (
      <Link href={`/discounts/${d.id}`} weight="semibold" color="primary">
        {d.title}
      </Link>
    ),
  },
  {
    key: "method",
    header: "Method",
    width: pixel(130),
    renderCell: (d) => <Text>{methodLabel(d)}</Text>,
  },
  {
    key: "summary",
    header: "Gives",
    width: proportional(4, { minWidth: 260 }),
    renderCell: (d) => (
      <Text type="supporting" maxLines={2}>
        {d.summary}
      </Text>
    ),
  },
  {
    key: "phase",
    header: "Status",
    width: pixel(120),
    renderCell: (d) => <DiscountStatusBadge phase={d.phase} />,
  },
  {
    key: "window",
    header: "When",
    width: pixel(170),
    renderCell: (d) => <ListWindow list={d} />,
  },
  {
    key: "used",
    header: "Uses",
    width: pixel(110),
    align: "end",
    renderCell: (d) => <Text>{usesLabel(d)}</Text>,
  },
];

export function DiscountsTable({
  page,
  canWrite,
}: {
  /** The page of discounts; absent while it loads (the toolbar shows meanwhile). */
  page?: { items: Discount[]; next_cursor: string | null; error?: string };
  canWrite: boolean;
}) {
  return (
    <ResourceTable<Discount>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={COLUMNS}
      idKey="id"
      rowHref={(d) => `/discounts/${d.id}`}
      isSelectable={false}
      filters={FILTERS}
      views={VIEWS}
      error={page?.error}
      empty={{
        icon: <ReceiptPercentIcon width={32} />,
        title: "No discounts yet",
        description:
          "A code for a newsletter, free shipping over an amount, buy one get one: start from a template.",
        action: canWrite ? <Button label="Create discount" href="/discounts/new" /> : undefined,
      }}
      noResults={{ title: "No discounts match these filters" }}
    />
  );
}
