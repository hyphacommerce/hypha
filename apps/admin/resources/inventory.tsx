"use client";

// The inventory list's table: an item per variant, its stock summed over locations, or
// at one location when the list is filtered by it (the API filters and sorts by that
// location's count, so the columns show the same one).

import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ArchiveBoxIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { LOW_STOCK } from "@/lib/inventory";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

type Item = AdminSchemas["InventoryItemSummaryResponse"];

const NO_NAMES: Record<string, string> = {};

const BELOW = [1, LOW_STOCK, 10, 25, 100];

/** The item's counts at `location` (zero where it has no level there), or its totals. */
function countsAt(item: Item, location: string | undefined) {
  if (!location) return item;
  return (
    item.levels.find((l) => l.location === location) ?? { on_hand: 0, reserved: 0, available: 0 }
  );
}

/** Units on their way to `location`, or to every location. */
function incomingAt(item: Item, location: string | undefined): number {
  return item.levels
    .filter((l) => !location || l.location === location)
    .reduce((n, l) => n + l.incoming, 0);
}

export function InventoryTable({
  page,
  locations = NO_NAMES,
  location,
}: {
  /** The page of rows; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Item[]; next_cursor: string | null; error?: string };
  /** Locations' names by ID. */
  locations?: Record<string, string>;
  /** The location the list is filtered by: the counts shown are that location's. */
  location?: string;
}) {
  const t = useT<"inventory">();
  const format = useFormat();
  const views: View[] = [
    { label: t("common.all"), params: {} },
    { label: t("inventory.view_low"), params: { below: [String(LOW_STOCK)] } },
    { label: t("inventory.view_out"), params: { below: ["1"] } },
  ];
  const filters: FilterDef[] = [
    {
      kind: "search",
      param: "sku",
      label: t("common.sku"),
      placeholder: t("inventory.filter_sku_placeholder"),
    },
    {
      kind: "select",
      param: "below",
      label: t("inventory.filter_available"),
      options: BELOW.map((n) => ({
        value: String(n),
        label:
          n === 1 ? t("inventory.filter_below_none") : t("inventory.filter_below", { count: n }),
      })),
    },
    {
      kind: "select",
      param: "location",
      label: t("common.location"),
      options: Object.entries(locations).map(([value, label]) => ({ value, label })),
    },
  ];
  const columns: TableColumn<Item>[] = [
    {
      key: "product",
      header: t("common.product"),
      renderCell: (item) => (
        <VStack gap={0.5} hAlign="start">
          {item.product ? (
            <Link href={`/products/${item.product}`} weight="semibold" color="primary">
              {item.product_title ?? item.product}
            </Link>
          ) : (
            <Text color="secondary">{t("inventory.removed_product")}</Text>
          )}
          {item.variant_title && (
            <Text type="supporting" color="secondary">
              {item.variant_title}
            </Text>
          )}
        </VStack>
      ),
    },
    {
      key: "sku",
      header: t("common.sku"),
      width: pixel(160),
      renderCell: (item) =>
        item.sku ? <Text type="code">{item.sku}</Text> : <Text color="secondary">—</Text>,
    },
    {
      key: "on_hand",
      header: t("inventory.on_hand"),
      width: pixel(100),
      align: "end",
      renderCell: (item) => (
        <Text hasTabularNumbers>{format.number(countsAt(item, location).on_hand)}</Text>
      ),
    },
    {
      key: "reserved",
      header: t("inventory.reserved"),
      width: pixel(100),
      align: "end",
      renderCell: (item) => (
        <Text hasTabularNumbers>{format.number(countsAt(item, location).reserved)}</Text>
      ),
    },
    {
      key: "incoming",
      header: t("inventory.incoming"),
      width: pixel(100),
      align: "end",
      renderCell: (item) => {
        const incoming = incomingAt(item, location);
        return incoming > 0 ? (
          <Text hasTabularNumbers>{format.number(incoming)}</Text>
        ) : (
          <Text color="secondary">—</Text>
        );
      },
    },
    {
      key: "available",
      header: t("inventory.available"),
      width: pixel(150),
      align: "end",
      renderCell: (item) => {
        const { available } = countsAt(item, location);
        return !item.tracked ? (
          <Badge label={t("inventory.not_tracked")} variant="neutral" />
        ) : available <= 0 ? (
          <Badge
            label={
              item.continue_selling
                ? t("inventory.available_still_selling", { count: available })
                : format.number(available)
            }
            variant="error"
          />
        ) : available < LOW_STOCK ? (
          <Badge label={format.number(available)} variant="warning" />
        ) : (
          <Text hasTabularNumbers>{format.number(available)}</Text>
        );
      },
    },
  ];
  return (
    <ResourceTable<Item>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(item) =>
        item.product ? `/inventory/${item.id}?product=${item.product}` : `/inventory/${item.id}`
      }
      isSelectable={false}
      views={views}
      filters={filters}
      error={page?.error}
      empty={{
        icon: <ArchiveBoxIcon width={32} />,
        title: t("inventory.list_empty_title"),
        description: t("inventory.list_empty_description"),
      }}
      noResults={{
        title: t("inventory.no_results_title"),
        description: t("inventory.list_no_results_description"),
      }}
    />
  );
}
