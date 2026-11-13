import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { DetailSkeleton } from "@/components/skeletons";
import { formatUnitCost } from "@/lib/format";
import {
  loadCurrencies,
  loadStore,
  loaded,
  locationNames,
  requireStaff,
  sessionClient,
} from "@/lib/hypha";
import { STOCK_REASONS, incomingLabel } from "@/lib/inventory";
import { hasScope } from "@/lib/navigation";

import { AdjustStock } from "./adjust-stock";
import { HistoryTable } from "./history-table";
import { SetCost } from "./set-cost";

const BACK: Crumb = { label: "Inventory", href: "/inventory" };

/**
 * One variant's stock: its counts per location, adjusting them for a reason, and its
 * history. `?product=` (from the links that know it) names the variant in the header
 * without waiting for the item; otherwise the item's own product does.
 */
export default async function InventoryItemPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ id }, query] = await Promise.all([params, searchParams]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1080}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <ItemDetails id={id} product={one(query, "product")} />
        </Suspense>
        <Suspense fallback={<HistoryTable />}>
          <History id={id} params={query} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ItemDetails({ id, product }: { id: string; product?: string }) {
  const client = await sessionClient();
  const [item, locations, me, store, currencies, owner] = await Promise.all([
    client.GET("/admin/inventory-items/{id}", { params: { path: { id } } }).then(loaded),
    locationNames(client),
    requireStaff(),
    loadStore(),
    loadCurrencies(),
    product
      ? client.GET("/admin/products/{id}", { params: { path: { id: product } } })
      : Promise.resolve(undefined),
  ]);
  // Opened without `?product=` (a history link, a bookmark): the item names its product.
  const named =
    owner ??
    (item.product
      ? await client.GET("/admin/products/{id}", { params: { path: { id: item.product } } })
      : undefined);
  const found = named?.data;
  const variant = found?.variants.find((v) => v.id === item.variant);
  const title = found && variant ? found.title : "Stock";
  const back: Crumb[] =
    found && variant ? [BACK, { label: found.title, href: `/products/${found.id}` }] : [BACK];
  return (
    <>
      <PageHeader
        title={title}
        back={back}
        description={
          [variant?.title, variant?.sku, item.tracked ? null : "Not tracked"]
            .filter(Boolean)
            .join(" · ") || undefined
        }
        actions={
          hasScope(me.scopes, "inventory:write") && (
            <AdjustStock item={id} locations={[...locations]} />
          )
        }
      />
      <Card>
        <MetadataList title="On hand">
          {item.levels.length === 0 ? (
            <Text color="secondary">No stock at any location yet.</Text>
          ) : (
            item.levels.map((level) => (
              <MetadataListItem
                key={level.location}
                label={locations.get(level.location) ?? "Removed location"}
              >
                <Text hasTabularNumbers>
                  {level.on_hand} on hand · {level.reserved} reserved · {level.available} available
                  {incomingLabel(level) && ` · ${incomingLabel(level)}`}
                </Text>
              </MetadataListItem>
            ))
          )}
        </MetadataList>
      </Card>
      {/* Absent without purchasing:read: cost is for those who buy. */}
      {item.unit_cost !== undefined && (
        <Card>
          <MetadataList title="Cost">
            <MetadataListItem label="Unit cost">
              <Text hasTabularNumbers>
                {item.unit_cost ? formatUnitCost(item.unit_cost, currencies) : "Not set"}
              </Text>
            </MetadataListItem>
          </MetadataList>
          {hasScope(me.scopes, "purchasing:write") &&
            (store?.currency ? (
              <SetCost item={id} cost={item.unit_cost} currency={store.currency} />
            ) : (
              <Text type="supporting" color="secondary">
                Choose the store's base currency in Settings → Store to record costs.
              </Text>
            ))}
        </Card>
      )}
    </>
  );
}

async function History({ id, params }: { id: string; params: SearchParams }) {
  const client = await sessionClient();
  const [result, locations, me] = await Promise.all([
    client.GET("/admin/inventory-items/{id}/movements", {
      params: {
        path: { id },
        query: {
          ...pageQuery(params),
          location: one(params, "location"),
          reason: oneOf(params, "reason", STOCK_REASONS),
        },
      },
    }),
    locationNames(client),
    requireStaff(),
  ]);
  return (
    <VStack gap={3}>
      <Text type="display-3" as="h2">
        History
      </Text>
      <HistoryTable
        page={await listPage(result, `/inventory/${id}`, params)}
        locations={Object.fromEntries(locations)}
        showCost={hasScope(me.scopes, "purchasing:read")}
      />
    </VStack>
  );
}
