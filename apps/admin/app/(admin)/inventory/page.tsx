import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { locationNames, sessionClient } from "@/lib/hypha";
import { InventoryTable } from "@/resources/inventory";

/**
 * Stock across products: every variant's inventory item with what's on hand, reserved
 * and available, filtered by how little is left (`below`), a location or a SKU. The
 * header and the toolbar render at once; the items stream in.
 */
export default async function InventoryPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Inventory"
          description="Stock of every variant. Change it on the product's page."
        />
        <Suspense fallback={<InventoryTable />}>
          <InventoryRows params={params} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function InventoryRows({ params }: { params: SearchParams }) {
  const client = await sessionClient();
  const below = Number(one(params, "below"));
  const location = one(params, "location");
  const [result, locations] = await Promise.all([
    client.GET("/admin/inventory-items", {
      params: {
        query: {
          ...pageQuery(params),
          below: Number.isSafeInteger(below) ? below : undefined,
          location,
          // Only from a link (`?product=`): no chip offers it.
          product: one(params, "product"),
          sku: one(params, "sku"),
        },
      },
    }),
    locationNames(client),
  ]);
  return (
    <InventoryTable
      page={listPage(result, "/inventory", params)}
      locations={Object.fromEntries(locations)}
      location={location}
    />
  );
}
