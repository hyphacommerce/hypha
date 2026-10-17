"use client";

// The product list's table (design note 0024, §3): its columns, sorts and filters.

import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { CubeIcon, PhotoIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";

import {
  type FilterDef,
  ResourceTable,
  type SortDef,
  type SortOption,
} from "@/components/resource-table/resource-table";
import { ProductStatusBadge } from "@/components/status";
import { type Currencies, formatDateTime, formatMoney } from "@/lib/format";

type Product = AdminSchemas["ProductSummaryResponse"];

const SORT_OPTIONS: SortOption[] = [
  { value: "newest", label: "Newest" },
  { value: "oldest", label: "Oldest" },
  { value: "updated_desc", label: "Recently changed" },
  { value: "updated_asc", label: "Least recently changed" },
  { value: "title_asc", label: "Title A–Z" },
  { value: "title_desc", label: "Title Z–A" },
];

const SORTS: Record<string, SortDef> = {
  title: { asc: "title_asc", desc: "title_desc" },
  updated: { asc: "updated_asc", desc: "updated_desc" },
};

const STATUSES = [
  { value: "active", label: "Active" },
  { value: "draft", label: "Draft" },
  { value: "archived", label: "Archived" },
];

/** "€9.50", "€9.50 – €14.00" for several prices, one range per currency. */
function priceRanges(product: Product, currencies: Currencies): string {
  if (product.price_ranges.length === 0) return "—";
  return product.price_ranges
    .map(({ min, max }) =>
      min.amount === max.amount
        ? formatMoney(min, currencies)
        : `${formatMoney(min, currencies)} – ${formatMoney(max, currencies)}`,
    )
    .join(", ");
}

/** Names, at most two then "+N". */
function names(ids: string[], name: Record<string, string>): string {
  const shown = ids.slice(0, 2).map((id) => name[id] ?? id);
  return ids.length > 2 ? `${shown.join(", ")} +${ids.length - 2}` : shown.join(", ");
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
        // A plain <img>: the media URLs resize, so no Next.js image pipeline (0011 §2).
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
  currencies,
  channels,
  categories,
}: {
  page: { items: Product[]; next_cursor: string | null; error?: string };
  currencies: Currencies;
  /** Channel names by ID. */
  channels: Record<string, string>;
  /** Category paths by ID ("Clothing › Shirts"). */
  categories: Record<string, string>;
}) {
  const shortName = Object.fromEntries(
    Object.entries(categories).map(([id, path]) => [id, path.split(" › ").at(-1) ?? path]),
  );
  const filters: FilterDef[] = [
    { kind: "search", param: "q", label: "Search", placeholder: "Search by title, handle or SKU" },
    { kind: "multi", param: "status", label: "Status", options: STATUSES },
    {
      kind: "select",
      param: "category",
      label: "Category",
      options: Object.entries(categories)
        .map(([value, label]) => ({ value, label }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    },
    {
      kind: "select",
      param: "channel",
      label: "Channel",
      options: Object.entries(channels).map(([value, label]) => ({ value, label })),
    },
    { kind: "text", param: "vendor", label: "Vendor" },
    { kind: "text", param: "product_type", label: "Type" },
    { kind: "text", param: "tag", label: "Tag" },
  ];
  const columns: TableColumn<Product>[] = [
    {
      key: "title",
      header: "Product",
      width: proportional(3, { minWidth: 240 }),
      renderCell: (product) => (
        <HStack gap={3} vAlign="center">
          <FeaturedImage media={product.featured_media} />
          <VStack gap={0.5}>
            <Link href={`/products/${product.id}`} weight="medium">
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
      header: "Status",
      width: pixel(110),
      renderCell: (product) => <ProductStatusBadge status={product.status} />,
    },
    {
      key: "channels",
      header: "Channels",
      renderCell: (product) =>
        product.channels.length === 0 ? (
          <Text color="secondary">None</Text>
        ) : (
          names(product.channels, channels)
        ),
    },
    {
      key: "categories",
      header: "Categories",
      renderCell: (product) =>
        product.categories.length === 0 ? (
          <Text color="secondary">None</Text>
        ) : (
          names(product.categories, shortName)
        ),
    },
    { key: "variant_count", header: "Variants", width: pixel(90), align: "end" },
    {
      key: "price",
      header: "Price",
      renderCell: (product) => priceRanges(product, currencies),
    },
    {
      key: "updated",
      header: "Changed",
      width: pixel(170),
      renderCell: (product) => <Text color="secondary">{formatDateTime(product.updated_at)}</Text>,
    },
  ];
  return (
    <ResourceTable<Product>
      rows={page.items}
      nextCursor={page.next_cursor}
      columns={columns}
      idKey="id"
      rowHref={(p) => `/products/${p.id}`}
      filters={filters}
      sorts={SORTS}
      sortOptions={SORT_OPTIONS}
      defaultSort="newest"
      error={page.error}
      empty={{
        icon: <CubeIcon width={32} />,
        title: "No products yet",
        description: "Products you add show up here, newest first.",
        action: <Button label="Add product" href="/products/new" />,
      }}
      noResults={{
        title: "No products match these filters",
        description: "Search looks at titles, handles and SKUs; vendor, type and tag match whole.",
      }}
    />
  );
}
