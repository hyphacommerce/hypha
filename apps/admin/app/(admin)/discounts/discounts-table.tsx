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
import type { Format } from "@/lib/format";
import type { TFor } from "@/lib/messages";
import { PHASES } from "@/lib/price-lists";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { ListWindow } from "../markets/price-lists/price-lists-table";

type DiscountsT = TFor<"common" | "discounts">;

const filters = (t: DiscountsT): FilterDef[] => [
  {
    kind: "search",
    param: "q",
    label: t("common.search"),
    placeholder: t("discounts.list_search_placeholder"),
  },
  {
    kind: "select",
    param: "status",
    label: t("common.status"),
    options: PHASES.map((p) => ({ value: p.value, label: p.label })),
  },
  {
    kind: "select",
    param: "method",
    label: t("discounts.list_filter_method"),
    options: [
      { value: "codes", label: t("discounts.list_method_codes") },
      { value: "automatic", label: t("discounts.list_method_automatic") },
    ],
  },
  {
    kind: "select",
    param: "class",
    label: t("discounts.list_filter_class"),
    options: [
      { value: "product", label: t("common.products") },
      { value: "order", label: t("discounts.list_class_order") },
      { value: "shipping", label: t("common.shipping") },
    ],
  },
];

const views = (t: DiscountsT): View[] => [
  { label: t("common.all"), params: {} },
  { label: t("common.active"), params: { status: ["active"] } },
  { label: t("discounts.list_view_scheduled"), params: { status: ["scheduled"] } },
  { label: t("discounts.list_view_codes"), params: { method: ["codes"] } },
  { label: t("discounts.list_view_automatic"), params: { method: ["automatic"] } },
];

const columns = (t: DiscountsT, format: Format): TableColumn<Discount>[] => [
  {
    key: "title",
    header: t("discounts.list_column_discount"),
    width: proportional(2, { minWidth: 200 }),
    renderCell: (d) => (
      <Link href={`/discounts/${d.id}`} weight="semibold" color="primary">
        {d.title}
      </Link>
    ),
  },
  {
    key: "method",
    header: t("discounts.list_column_method"),
    width: pixel(130),
    renderCell: (d) => <Text>{methodLabel(d, t)}</Text>,
  },
  {
    key: "summary",
    header: t("discounts.list_column_gives"),
    width: proportional(4, { minWidth: 260 }),
    renderCell: (d) => (
      <Text type="supporting" maxLines={2}>
        {d.summary}
      </Text>
    ),
  },
  {
    key: "phase",
    header: t("common.status"),
    width: pixel(120),
    renderCell: (d) => <DiscountStatusBadge phase={d.phase} />,
  },
  {
    key: "window",
    header: t("discounts.list_column_when"),
    width: pixel(170),
    renderCell: (d) => <ListWindow list={d} />,
  },
  {
    key: "used",
    header: t("discounts.list_column_uses"),
    width: pixel(110),
    align: "end",
    renderCell: (d) => <Text>{usesLabel(d, t, format)}</Text>,
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
  const t = useT<"discounts">();
  const format = useFormat();
  return (
    <ResourceTable<Discount>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns(t, format)}
      idKey="id"
      rowHref={(d) => `/discounts/${d.id}`}
      isSelectable={false}
      filters={filters(t)}
      views={views(t)}
      error={page?.error}
      empty={{
        icon: <ReceiptPercentIcon width={32} />,
        title: t("discounts.list_empty_title"),
        description: t("discounts.list_empty_description"),
        action: canWrite ? (
          <Button label={t("discounts.create")} href="/discounts/new" />
        ) : undefined,
      }}
      noResults={{ title: t("discounts.list_no_results") }}
    />
  );
}
