import { Badge } from "@astryxdesign/core/Badge";
import { Card } from "@astryxdesign/core/Card";
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
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";

const PERCENT = new Intl.NumberFormat("en-IE", {
  maximumFractionDigits: 2,
  signDisplay: "always",
});

/** "−10%" from basis points (−1000); "No adjustment" for 0. */
function adjustment(bps: number): string {
  if (bps === 0) return "No adjustment";
  return `${PERCENT.format(bps / 100)}%`;
}

/**
 * Every price list, with the markets whose catalog uses it: a list is edited on its
 * market's page, so each row links there. The header renders at once; the lists stream in.
 */
export default function PriceListsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Price lists"
          description="How each market's prices differ from the base prices: an adjustment, and fixed prices for some variants. Edit one on its market's page."
          back={{ label: "Markets", href: "/markets" }}
        />
        <Suspense fallback={<TableSkeleton rows={4} />}>
          <PriceListRows />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function PriceListRows() {
  const client = await sessionClient();
  const [lists, markets] = await Promise.all([
    client.GET("/admin/price-lists").then(loaded),
    client.GET("/admin/markets").then(loaded),
  ]);
  // A store has a handful of markets, so reading each catalog is cheap.
  const catalogs = await Promise.all(
    markets.items
      .filter((m) => m.catalog)
      .map((m) =>
        client
          .GET("/admin/catalogs/{id}", { params: { path: { id: m.catalog ?? "" } } })
          .then((r) => ({ market: m, list: r.data?.price_list ?? null })),
      ),
  );
  const usedBy = (list: string) => catalogs.filter((c) => c.list === list).map((c) => c.market);

  return (
    <Card padding={0}>
      <Table hasHover>
        <TableHeader>
          <TableRow>
            <TableHeaderCell>Used by</TableHeaderCell>
            <TableHeaderCell>Currency</TableHeaderCell>
            <TableHeaderCell>Adjustment</TableHeaderCell>
          </TableRow>
        </TableHeader>
        <TableBody>
          {lists.items.map((list) => {
            const users = usedBy(list.id);
            return (
              <TableRow key={list.id}>
                <TableCell>
                  {users.length > 0 ? (
                    <HStack gap={2} wrap="wrap">
                      {users.map((m) => (
                        <Link key={m.id} href={`/markets/${m.id}`} weight="medium">
                          {m.name}
                        </Link>
                      ))}
                    </HStack>
                  ) : (
                    <HStack gap={2} vAlign="center">
                      <Badge label="Not used" variant="neutral" />
                      <Text type="supporting" color="secondary">
                        {list.id}
                      </Text>
                    </HStack>
                  )}
                </TableCell>
                <TableCell>{list.currency}</TableCell>
                <TableCell>{adjustment(list.adjustment_bps)}</TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
      {lists.items.length === 0 && (
        <div style={{ padding: 16 }}>
          <Text color="secondary">
            No price lists yet. A market gets one when you set its prices.
          </Text>
        </div>
      )}
    </Card>
  );
}
