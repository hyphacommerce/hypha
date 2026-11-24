"use client";

// The product list's table: its columns, sorts and filters.

import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { CubeIcon, PhotoIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState } from "react";

import { customFieldFilters } from "@/components/custom-fields/filters";
import { DateTime } from "@/components/date-time";
import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
  type View,
} from "@/components/resource-table/resource-table";
import { ProductStatusBadge, statusLabel, statusOptions } from "@/components/status";
import type { Definition } from "@/lib/custom-fields";
import type { KeyIn, TFor } from "@/lib/messages";
import { type Format, useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { type BulkOutcome, BulkOutcomeBanner, ProductBulkActions } from "./bulk";

/** No filterable custom fields. */
const NO_FIELDS: Definition[] = [];
const NO_NAMES: Record<string, string> = {};

type Product = AdminSchemas["ProductSummaryResponse"];

const SORT_OPTIONS: { value: string; label: KeyIn<"common" | "products"> }[] = [
  { value: "newest", label: "common.sort_newest" },
  { value: "oldest", label: "common.sort_oldest" },
  { value: "updated_desc", label: "products.list_sort_updated_desc" },
  { value: "updated_asc", label: "products.list_sort_updated_asc" },
  { value: "title_asc", label: "products.list_sort_title_asc" },
  { value: "title_desc", label: "products.list_sort_title_desc" },
];

const SORTS: Record<string, SortDef> = {
  title: { asc: "title_asc", desc: "title_desc" },
  updated: { asc: "updated_asc", desc: "updated_desc" },
};

const STATUSES = ["active", "draft", "archived"] as const;

/** "€9.50", "€9.50 – €14.00" for several prices, one range per currency. */
function priceRanges(product: Product, t: TFor<"products" | "common">, format: Format): string {
  if (product.price_ranges.length === 0) return t("common.empty_value");
  return format.list(
    product.price_ranges.map(({ min, max }) =>
      min.amount === max.amount
        ? format.money(min)
        : t("products.price_range", { min: format.money(min), max: format.money(max) }),
    ),
    "unit",
  );
}

/** Names, at most two then "+N". */
function names(
  ids: string[],
  name: Record<string, string>,
  t: TFor<"products">,
  format: Format,
): string {
  const shown = format.list(
    ids.slice(0, 2).map((id) => name[id] ?? id),
    "unit",
  );
  return ids.length > 2
    ? t("products.list_names_more", { names: shown, count: ids.length - 2 })
    : shown;
}

/** The product's featured image, small; a placeholder for none (or a video). */
function FeaturedImage({ media }: { media?: AdminSchemas["ProductMediaResponse"] | null }) {
  const src = media?.kind === "image" ? (media.thumbnail_url ?? media.url) : undefined;
  return (
    <div
      style={{
        width: 40,
        height: 40,
        flexShrink: 0,
        borderRadius: 6,
        overflow: "hidden",
        display: "grid",
        placeItems: "center",
        background: "var(--color-background-muted)",
      }}
    >
      {src ? (
        // A plain <img>: the media URLs resize, so no Next.js image pipeline.
        <img src={src} alt="" width={40} height={40} style={{ objectFit: "cover" }} />
      ) : (
        <Text color="secondary">
          <PhotoIcon width={18} aria-hidden />
        </Text>
      )}
    </div>
  );
}

export function ProductsTable({
  page,
  channels = NO_NAMES,
  categories = NO_NAMES,
  fieldFilters = NO_FIELDS,
}: {
  /** The page of rows; absent while it loads (the table shows its toolbar meanwhile). */
  page?: { items: Product[]; next_cursor: string | null; error?: string };
  /** Filterable custom fields, offered as filters. */
  fieldFilters?: Definition[];
  /** Channel names by ID. */
  channels?: Record<string, string>;
  /** Category paths by ID ("Clothing › Shirts"). */
  categories?: Record<string, string>;
}) {
  const t = useT<"products">();
  const format = useFormat();
  const [outcome, setOutcome] = useState<BulkOutcome>();
  const shortName = Object.fromEntries(
    Object.entries(categories).map(([id, path]) => [id, path.split(" › ").at(-1) ?? path]),
  );
  const filters: FilterDef[] = [
    {
      kind: "search",
      param: "q",
      label: t("common.search"),
      placeholder: t("products.list_search_placeholder"),
    },
    {
      kind: "multi",
      param: "status",
      label: t("common.status"),
      options: statusOptions("product", STATUSES, t),
    },
    {
      kind: "select",
      param: "category",
      label: t("common.category"),
      options: Object.entries(categories)
        .map(([value, label]) => ({ value, label }))
        .toSorted((a, b) => format.compare(a.label, b.label)),
    },
    {
      kind: "select",
      param: "channel",
      label: t("common.channel"),
      options: Object.entries(channels).map(([value, label]) => ({ value, label })),
    },
    { kind: "text", param: "vendor", label: t("products.vendor") },
    { kind: "text", param: "product_type", label: t("common.type") },
    { kind: "text", param: "tag", label: t("products.tag") },
    ...customFieldFilters(fieldFilters, t),
  ];
  const views: View[] = [
    { label: t("common.all"), params: {} },
    ...STATUSES.map((status) => ({
      label: statusLabel("product", status, t),
      params: { status: [status] },
    })),
  ];
  const sortOptions: SortOption[] = SORT_OPTIONS.map(({ value, label }) => ({
    value,
    label: t(label),
  }));
  const columns: TableColumn<Product>[] = [
    {
      key: "title",
      header: t("common.product"),
      width: proportional(3, { minWidth: 240 }),
      renderCell: (product) => (
        <HStack gap={3} vAlign="center">
          <FeaturedImage media={product.featured_media} />
          <VStack gap={0.5}>
            <Link href={`/products/${product.id}`} weight="semibold" color="primary">
              {product.title}
            </Link>
            <Text type="supporting" color="secondary">
              /{product.handle}
            </Text>
          </VStack>
        </HStack>
      ),
    },
    {
      key: "status",
      header: t("common.status"),
      width: pixel(110),
      renderCell: (product) => <ProductStatusBadge status={product.status} t={t} />,
    },
    {
      key: "channels",
      header: t("common.channels"),
      renderCell: (product) =>
        product.channels.length === 0 ? (
          <Text color="secondary">{t("common.empty_value")}</Text>
        ) : (
          names(product.channels, channels, t, format)
        ),
    },
    {
      key: "categories",
      header: t("common.categories"),
      renderCell: (product) =>
        product.categories.length === 0 ? (
          <Text color="secondary">{t("common.empty_value")}</Text>
        ) : (
          names(product.categories, shortName, t, format)
        ),
    },
    {
      key: "variant_count",
      header: t("common.variants"),
      width: pixel(90),
      align: "end",
      renderCell: (product) => format.number(product.variant_count),
    },
    {
      key: "price",
      header: t("common.price"),
      renderCell: (product) => priceRanges(product, t, format),
    },
    {
      key: "updated",
      header: t("products.list_column_changed"),
      width: pixel(170),
      renderCell: (product) => (
        <Text color="secondary">
          <DateTime value={product.updated_at} short />
        </Text>
      ),
    },
  ];
  return (
    <VStack gap={3}>
      {outcome && <BulkOutcomeBanner outcome={outcome} onClose={() => setOutcome(undefined)} />}
      <ResourceTable<Product>
        renderBulkActions={(selected, clear) => (
          <ProductBulkActions
            selected={selected}
            clear={clear}
            channels={channels}
            categories={categories}
            onDone={setOutcome}
          />
        )}
        rowLabel={(p) => p.title}
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns}
        idKey="id"
        rowHref={(p) => `/products/${p.id}`}
        filters={filters}
        views={views}
        sorts={SORTS}
        sortOptions={sortOptions}
        defaultSort="newest"
        error={page?.error}
        empty={{
          icon: <CubeIcon width={32} />,
          title: t("products.list_empty_title"),
          description: t("products.list_empty_description"),
          action: <Button label={t("products.add_product")} href="/products/new" />,
        }}
        noResults={{
          title: t("products.list_no_results_title"),
          description: t("products.list_no_results_description"),
        }}
      />
    </VStack>
  );
}
