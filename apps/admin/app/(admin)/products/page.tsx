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
import { CubeIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";

import { PageHeader } from "@/components/page-header";
import { Pager } from "@/components/pager";
import { ProductStatusBadge } from "@/components/status";
import { formatMoney } from "@/lib/format";
import { loaded, sessionClient } from "@/lib/hypha";

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

/** "€9.50" for one variant, "€9.50 – €14.00" for several prices. */
function priceRange(product: AdminSchemas["ProductResponse"]): string {
  const amounts = product.variants.map((v) => v.price.amount);
  const first = product.variants[0];
  if (!first) return "—";
  const low = { ...first.price, amount: Math.min(...amounts) };
  const high = { ...first.price, amount: Math.max(...amounts) };
  return low.amount === high.amount
    ? formatMoney(low)
    : `${formatMoney(low)} – ${formatMoney(high)}`;
}

export default async function ProductsPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const status = asStatus(params.status);
  const cursor = typeof params.cursor === "string" ? params.cursor : undefined;

  const client = await sessionClient();
  const result = await client.GET("/admin/products", {
    params: { query: { status, cursor } },
  });
  // A hand-edited URL (an old or foreign cursor): start the list again, same filter.
  if (result.response.status === 400) {
    redirect(status ? `/products?status=${status}` : "/products");
  }
  const page = loaded(result);

  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Products"
          actions={<Button label="Add product" variant="primary" href="/products/new" />}
        />

        <HStack gap={1}>
          {FILTERS.map((filter) => (
            <Button
              key={filter.label}
              label={filter.label}
              size="sm"
              variant={filter.status === status ? "secondary" : "ghost"}
              href={filter.status ? `/products?status=${filter.status}` : "/products"}
            />
          ))}
        </HStack>

        {page.items.length === 0 ? (
          <EmptyState
            icon={<CubeIcon width={32} />}
            title={
              cursor ? "No more products" : status ? `No ${status} products` : "No products yet"
            }
            description="Products you add show up here, newest first."
            actions={<Button label="Add product" href="/products/new" />}
          />
        ) : (
          <Table hasHover>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Product</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>Variants</TableHeaderCell>
                <TableHeaderCell>Price</TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.items.map((product) => (
                <TableRow key={product.id}>
                  <TableCell>
                    <VStack gap={0.5}>
                      <Link href={`/products/${product.id}`} weight="medium">
                        {product.title}
                      </Link>
                      <Text type="supporting" color="secondary">
                        /{product.handle}
                      </Text>
                    </VStack>
                  </TableCell>
                  <TableCell>
                    <ProductStatusBadge status={product.status} />
                  </TableCell>
                  <TableCell>{product.variants.length}</TableCell>
                  <TableCell>{priceRange(product)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}

        <Pager
          path="/products"
          query={{ status }}
          isFirstPage={!cursor}
          nextCursor={page.next_cursor}
        />
      </VStack>
    </Section>
  );
}
