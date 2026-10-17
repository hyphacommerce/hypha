import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Section } from "@astryxdesign/core/Section";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { CubeIcon, PhotoIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";

import { PageHeader } from "@/components/page-header";
import { Pager } from "@/components/pager";
import { ProductStatusBadge } from "@/components/status";
import { type Currencies, formatMoney } from "@/lib/format";
import { pathsById } from "@/lib/categories";
import { channelNames, loadCurrencies, loaded, sessionClient } from "@/lib/hypha";

import { SearchBox } from "./search-box";

type Status = AdminSchemas["ProductStatus"];
const FILTERS: { label: string; status?: Status }[] = [
  { label: "All" },
  { label: "Active", status: "active" },
  { label: "Draft", status: "draft" },
  { label: "Archived", status: "archived" },
];

function asStatus(value: string | string[] | undefined): Status | undefined {
  return FILTERS.find((f) => f.status === value)?.status;
}

/** "€9.50", "€9.50 – €14.00" for several prices, one range per currency. */
function priceRanges(
  product: AdminSchemas["ProductSummaryResponse"],
  currencies: Currencies,
): string {
  if (product.price_ranges.length === 0) return "—";
  return product.price_ranges
    .map(({ min, max }) =>
      min.amount === max.amount
        ? formatMoney(min, currencies)
        : `${formatMoney(min, currencies)} – ${formatMoney(max, currencies)}`,
    )
    .join(", ");
}

/** A product's categories by name, at most two then "+N". */
function categoryNames(ids: string[], names: Map<string, string>): string {
  if (ids.length === 0) return "—";
  const shown = ids.slice(0, 2).map((id) => names.get(id)?.split(" › ").at(-1) ?? id);
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

export default async function ProductsPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const status = asStatus(params.status);
  const cursor = typeof params.cursor === "string" ? params.cursor : undefined;
  const q = typeof params.q === "string" && params.q.trim() !== "" ? params.q : undefined;
  const filters = (extra: Record<string, string | undefined> = {}) => {
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries({ status, q, ...extra })) {
      if (value) search.set(key, value);
    }
    const query = search.toString();
    return query ? `/products?${query}` : "/products";
  };

  const client = await sessionClient();
  const [result, channelName, currencies, categories] = await Promise.all([
    client.GET("/admin/products", { params: { query: { status, q, cursor } } }),
    channelNames(client),
    loadCurrencies(),
    client.GET("/admin/categories"),
  ]);
  // The tree once, for every row's category names.
  const categoryName = pathsById(categories.data?.items ?? []);
  // A hand-edited URL (an old or foreign cursor, text over 255 characters): start the
  // list again, same status.
  if (result.response.status === 400 || result.response.status === 422) {
    redirect(status ? `/products?status=${status}` : "/products");
  }
  const page = loaded(result);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Products"
          actions={
            <>
              <Button label="Categories" variant="secondary" href="/products/categories" />
              <Button label="Add product" variant="primary" href="/products/new" />
            </>
          }
        />

        <HStack gap={3} vAlign="center">
          <SearchBox key={q ?? ""} q={q ?? ""} status={status} />
          <HStack gap={1}>
            {FILTERS.map((filter) => (
              <Button
                key={filter.label}
                label={filter.label}
                size="sm"
                variant={filter.status === status ? "secondary" : "ghost"}
                href={filters({ status: filter.status })}
              />
            ))}
          </HStack>
        </HStack>

        {page.items.length === 0 ? (
          <EmptyState
            icon={<CubeIcon width={32} />}
            title={
              cursor
                ? "No more products"
                : q
                  ? `No products match “${q}”`
                  : status
                    ? `No ${status} products`
                    : "No products yet"
            }
            description={
              q
                ? "Search looks at titles, handles and SKUs."
                : "Products you add show up here, newest first."
            }
            actions={
              q ? (
                <Button label="Clear search" href={filters({ q: undefined })} />
              ) : (
                <Button label="Add product" href="/products/new" />
              )
            }
          />
        ) : (
          <Table hasHover>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Product</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>Channels</TableHeaderCell>
                <TableHeaderCell>Categories</TableHeaderCell>
                <TableHeaderCell>Variants</TableHeaderCell>
                <TableHeaderCell>Price</TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.items.map((product) => (
                <TableRow key={product.id}>
                  <TableCell>
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
                  </TableCell>
                  <TableCell>
                    <ProductStatusBadge status={product.status} />
                  </TableCell>
                  <TableCell>
                    {product.channels.length === 0 ? (
                      <Text color="secondary">None</Text>
                    ) : (
                      product.channels.map((id) => channelName.get(id) ?? id).join(", ")
                    )}
                  </TableCell>
                  <TableCell>
                    {product.categories.length === 0 ? (
                      <Text color="secondary">None</Text>
                    ) : (
                      categoryNames(product.categories, categoryName)
                    )}
                  </TableCell>
                  <TableCell>{product.variant_count}</TableCell>
                  <TableCell>{priceRanges(product, currencies)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}

        <Pager
          path="/products"
          query={{ status, q }}
          isFirstPage={!cursor}
          nextCursor={page.next_cursor}
        />
      </VStack>
    </Section>
  );
}
