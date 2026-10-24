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
  type PriceList,
  PHASES,
  adjustmentLabel,
  audienceLabel,
  kindLabel,
  phaseBadge,
} from "@/lib/price-lists";

const NO_NAMES: Record<string, string> = {};

const SORT_OPTIONS: SortOption[] = [
  { value: "newest", label: "Newest" },
  { value: "oldest", label: "Oldest" },
  { value: "name_asc", label: "Name A–Z" },
  { value: "name_desc", label: "Name Z–A" },
];

const SORTS: Record<string, SortDef> = { name: { asc: "name_asc", desc: "name_desc" } };

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Active", params: { status: ["active"] } },
  { label: "Scheduled", params: { status: ["scheduled"] } },
  { label: "Sales", params: { kind: ["sale"] } },
];

/** When the list prices anything: "Always", "From …", "Until …", or both. */
export function ListWindow({ list }: { list: Pick<PriceList, "starts_at" | "ends_at"> }) {
  if (!list.starts_at && !list.ends_at) return <Text color="secondary">Always</Text>;
  return (
    <VStack gap={0}>
      {list.starts_at && (
        <Text type="supporting">
          From <DateTime value={list.starts_at} short />
        </Text>
      )}
      {list.ends_at && (
        <Text type="supporting">
          Until <DateTime value={list.ends_at} short />
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
  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "kind",
      label: "Kind",
      options: [
        { value: "sale", label: "Sale" },
        { value: "override", label: "Regular prices" },
      ],
    },
    {
      kind: "select",
      param: "status",
      label: "Status",
      options: PHASES.map((p) => ({ value: p.value, label: p.label })),
    },
    {
      kind: "select",
      param: "market",
      label: "Market",
      options: Object.entries(markets)
        .map(([value, label]) => ({ value, label }))
        .toSorted((a, b) => a.label.localeCompare(b.label)),
    },
    {
      kind: "select",
      param: "group",
      label: "Customer group",
      options: Object.entries(groups)
        .map(([value, label]) => ({ value, label }))
        .toSorted((a, b) => a.label.localeCompare(b.label)),
    },
  ];
  const columns: TableColumn<PriceList>[] = [
    {
      key: "name",
      header: "Price list",
      width: proportional(3, { minWidth: 220 }),
      renderCell: (list) => (
        <VStack gap={0.5} hAlign="start">
          <Link href={`/markets/price-lists/${list.id}`} weight="semibold" color="primary">
            {list.name}
          </Link>
          <Text type="supporting" color="secondary">
            {list.description || adjustmentLabel(list.adjustment)}
          </Text>
        </VStack>
      ),
    },
    {
      key: "kind",
      header: "Kind",
      width: pixel(140),
      renderCell: (list) => (
        <Badge label={kindLabel(list.kind)} variant={list.kind === "sale" ? "pink" : "blue"} />
      ),
    },
    {
      key: "phase",
      header: "Status",
      width: pixel(120),
      renderCell: (list) => {
        const badge = phaseBadge(list.phase);
        return <Badge label={badge.label} variant={badge.variant} />;
      },
    },
    {
      key: "window",
      header: "When",
      width: pixel(170),
      renderCell: (list) => <ListWindow list={list} />,
    },
    {
      key: "audience",
      header: "For",
      width: proportional(1, { minWidth: 120 }),
      renderCell: (list) => (
        <Text color={list.audience.groups.length === 0 ? "secondary" : undefined}>
          {audienceLabel(list.audience, groups)}
        </Text>
      ),
    },
    {
      key: "markets",
      header: "Markets",
      width: proportional(2, { minWidth: 140 }),
      renderCell: (list) =>
        list.markets.length === 0 ? (
          <Text color="secondary">None</Text>
        ) : (
          <Text>{list.markets.map((m) => markets[m] ?? m).join(", ")}</Text>
        ),
    },
    { key: "currency", header: "Currency", width: pixel(96) },
    {
      key: "fixed_price_count",
      header: "Fixed prices",
      width: pixel(120),
      align: "end",
      renderCell: (list) => <Text>{list.fixed_price_count.toLocaleString("en")}</Text>,
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
      views={VIEWS}
      sorts={SORTS}
      sortOptions={SORT_OPTIONS}
      defaultSort="newest"
      error={page?.error}
      empty={{
        icon: <TagIcon width={32} />,
        title: "No price lists yet",
        description:
          "Without one, a market sells at the base prices. Make a list for a market's own prices, a sale or wholesale.",
        action: <Button label="New price list" href="/markets/price-lists/new" />,
      }}
      noResults={{ title: "No price lists match these filters" }}
    />
  );
}
