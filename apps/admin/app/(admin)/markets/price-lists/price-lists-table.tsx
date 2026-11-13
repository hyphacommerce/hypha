"use client";

// The price lists table: kind, phase, window, audience, markets and fixed prices,
// filtered and sorted by the API.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TagIcon } from "@heroicons/react/24/outline";

import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import {
  KINDS,
  type PriceList,
  adjustmentLabel,
  audienceLabel,
  kindLabel,
  phaseBadge,
  phaseOptions,
} from "@/lib/price-lists";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

const NO_NAMES: Record<string, string> = {};

const SORTS: Record<string, SortDef> = { name: { asc: "name_asc", desc: "name_desc" } };

/** When the list prices anything: "Always", "From …", "Until …", or both. */
export function ListWindow({ list }: { list: Pick<PriceList, "starts_at" | "ends_at"> }) {
  const t = useT<"price_lists">();
  if (!list.starts_at && !list.ends_at) {
    return <Text color="secondary">{t("price_lists.window_always")}</Text>;
  }
  return (
    <VStack gap={0}>
      {list.starts_at && (
        <Text type="supporting">
          {t.rich("price_lists.window_from", {
            time: <DateTime key="time" value={list.starts_at} short />,
          })}
        </Text>
      )}
      {list.ends_at && (
        <Text type="supporting">
          {t.rich("price_lists.window_until", {
            time: <DateTime key="time" value={list.ends_at} short />,
          })}
        </Text>
      )}
    </VStack>
  );
}

export function PriceListsTable({
  page,
  markets = NO_NAMES,
  groups = NO_NAMES,
}: {
  /** The page of lists; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: PriceList[]; next_cursor: string | null; error?: string };
  /** Market names by ID. */
  markets?: Record<string, string>;
  /** Customer group names by ID (none without `customers:read`). */
  groups?: Record<string, string>;
}) {
  const t = useT<"price_lists">();
  const format = useFormat();
  const named = (names: Record<string, string>) =>
    Object.entries(names)
      .map(([value, label]) => ({ value, label }))
      .toSorted((a, b) => format.compare(a.label, b.label));
  const sortOptions: SortOption[] = [
    { value: "newest", label: t("common.sort_newest") },
    { value: "oldest", label: t("common.sort_oldest") },
    { value: "name_asc", label: t("common.sort_name_asc") },
    { value: "name_desc", label: t("common.sort_name_desc") },
  ];
  const views: View[] = [
    { label: t("common.all"), params: {} },
    { label: t("price_lists.view_active"), params: { status: ["active"] } },
    { label: t("price_lists.view_scheduled"), params: { status: ["scheduled"] } },
    { label: t("price_lists.view_sales"), params: { kind: ["sale"] } },
  ];
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "kind",
      label: t("common.kind"),
      options: KINDS.map((kind) => ({ value: kind, label: t(`price_lists.kind_${kind}`) })),
    },
    {
      kind: "select",
      param: "status",
      label: t("common.status"),
      options: phaseOptions(t),
    },
    { kind: "select", param: "market", label: t("common.market"), options: named(markets) },
    {
      kind: "select",
      param: "group",
      label: t("price_lists.filter_group"),
      options: named(groups),
    },
  ];
  const columns: TableColumn<PriceList>[] = [
    {
      key: "name",
      header: t("price_lists.column_name"),
      width: proportional(3, { minWidth: 220 }),
      renderCell: (list) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/markets/price-lists/${list.id}`} weight="semibold" color="primary">
            {list.name}
          </Link>
          <Text type="supporting" color="secondary">
            {list.description || adjustmentLabel(t, format, list.adjustment)}
          </Text>
        </VStack>
      ),
    },
    {
      key: "kind",
      header: t("common.kind"),
      width: pixel(140),
      renderCell: (list) => (
        <Badge label={kindLabel(t, list.kind)} variant={list.kind === "sale" ? "pink" : "blue"} />
      ),
    },
    {
      key: "phase",
      header: t("common.status"),
      width: pixel(120),
      renderCell: (list) => {
        const badge = phaseBadge(list.phase, t);
        return <Badge label={badge.label} variant={badge.variant} />;
      },
    },
    {
      key: "window",
      header: t("price_lists.column_when"),
      width: pixel(170),
      renderCell: (list) => <ListWindow list={list} />,
    },
    {
      key: "audience",
      header: t("price_lists.column_for"),
      width: proportional(1, { minWidth: 120 }),
      renderCell: (list) => (
        <Text color={list.audience.groups.length === 0 ? "secondary" : undefined}>
          {audienceLabel(t, format, list.audience, groups)}
        </Text>
      ),
    },
    {
      key: "markets",
      header: t("common.markets"),
      width: proportional(2, { minWidth: 140 }),
      renderCell: (list) =>
        list.markets.length === 0 ? (
          <Text color="secondary">{t("common.none")}</Text>
        ) : (
          <Text>
            {format.list(
              list.markets.map((m) => markets[m] ?? m),
              "unit",
            )}
          </Text>
        ),
    },
    { key: "currency", header: t("common.currency"), width: pixel(96) },
    {
      key: "fixed_price_count",
      header: t("price_lists.column_fixed_prices"),
      width: pixel(120),
      align: "end",
      renderCell: (list) => <Text>{format.number(list.fixed_price_count)}</Text>,
    },
  ];
  return (
    <ResourceTable<PriceList>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(list) => `/markets/price-lists/${list.id}`}
      isSelectable={false}
      filters={filters}
      views={views}
      sorts={SORTS}
      sortOptions={sortOptions}
      defaultSort="newest"
      error={page?.error}
      empty={{
        icon: <TagIcon width={32} />,
        title: t("price_lists.list_empty_title"),
        description: t("price_lists.list_empty_description"),
        action: <Button label={t("price_lists.new")} href="/markets/price-lists/new" />,
      }}
      noResults={{ title: t("price_lists.list_no_results") }}
    />
  );
}
