"use client";

// The inventory list's table: an item per variant, its stock summed over locations.

import { Badge } from "@astryxdesign/core/Badge";
import { VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ArchiveBoxIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";

import {
  type FilterDef,
  ResourceTable,
  type View,
} from "@/components/resource-table/resource-table";
import { LOW_STOCK } from "@/lib/inventory";

type Item = AdminSchemas["InventoryItemSummaryResponse"];

const NO_NAMES: Record<string, string> = {};

const VIEWS: View[] = [
  { label: "All", params: {} },
  { label: "Running low", params: { below: [String(LOW_STOCK)] } },
  { label: "Out of stock", params: { below: ["1"] } },
];

const BELOW = [1, LOW_STOCK, 10, 25, 100];

export function InventoryTable({
  page,
  locations = NO_NAMES,
}: {
  /** The page of rows; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Item[]; next_cursor: string | null; error?: string };
  /** Locations' names by ID. */
  locations?: Record<string, string>;
}) {
  const filters: FilterDef[] = [
    { kind: "search", param: "sku", label: "SKU", placeholder: "Find a variant by its exact SKU" },
    {
      kind: "select",
      param: "below",
      label: "Available",
      options: BELOW.map((n) => ({
        value: String(n),
        label: n === 1 ? "None available" : `Fewer than ${n}`,
      })),
    },
    {
      kind: "select",
      param: "location",
      label: "Location",
      options: Object.entries(locations).map(([value, label]) => ({ value, label })),
    },
  ];
  const columns: TableColumn<Item>[] = [
    {
      key: "product",
      header: "Product",
      renderCell: (item) => (
        <VStack gap={0.5} hAlign="start">
          {item.product ? (
            <Link href={`/products/${item.product}`} weight="semibold" color="primary">
              {item.product_title ?? item.product}
            </Link>
          ) : (
            <Text color="secondary">Removed product</Text>
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
      header: "SKU",
      width: pixel(160),
      renderCell: (item) =>
        item.sku ? <Text type="code">{item.sku}</Text> : <Text color="secondary">—</Text>,
    },
    {
      key: "on_hand",
      header: "On hand",
      width: pixel(100),
      align: "end",
      renderCell: (item) => <Text hasTabularNumbers>{item.on_hand}</Text>,
    },
    {
      key: "reserved",
      header: "Reserved",
      width: pixel(100),
      align: "end",
      renderCell: (item) => <Text hasTabularNumbers>{item.reserved}</Text>,
    },
    {
      key: "available",
      header: "Available",
      width: pixel(150),
      align: "end",
      renderCell: (item) =>
        !item.tracked ? (
          <Badge label="Not tracked" variant="neutral" />
        ) : item.available <= 0 ? (
          <Badge
            label={item.continue_selling ? `${item.available}, still selling` : `${item.available}`}
            variant="error"
          />
        ) : item.available < LOW_STOCK ? (
          <Badge label={String(item.available)} variant="warning" />
        ) : (
          <Text hasTabularNumbers>{item.available}</Text>
        ),
    },
  ];
  return (
    <ResourceTable<Item>
      rows={page?.items}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="id"
      rowHref={(item) => (item.product ? `/products/${item.product}` : "/inventory")}
      isSelectable={false}
      views={VIEWS}
      filters={filters}
      error={page?.error}
      empty={{
        icon: <ArchiveBoxIcon width={32} />,
        title: "No stock yet",
        description: "Each variant's stock shows here once its product has variants.",
      }}
      noResults={{
        title: "Nothing here",
        description: "No variant matches: nothing is this low, or no SKU is exactly that.",
      }}
    />
  );
}
