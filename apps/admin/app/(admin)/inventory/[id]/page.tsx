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
import { getFormat } from "@/lib/get-format";
import { loadStore, loaded, locationNames, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { STOCK_REASONS, incomingLabel } from "@/lib/inventory";
import { hasScope } from "@/lib/navigation";

import { AdjustStock } from "./adjust-stock";
import { HistoryTable } from "./history-table";
import { SetCost } from "./set-cost";

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
  const [{ id }, query, t] = await Promise.all([params, searchParams, getT()]);
  const back: Crumb = { label: t("inventory.list_title"), href: "/inventory" };
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1080}>
        <Suspense fallback={<DetailSkeleton back={back} />}>
          <ItemDetails id={id} product={one(query, "product")} back={back} />
        </Suspense>
        <Suspense fallback={<HistoryTable />}>
          <History id={id} params={query} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ItemDetails({
  id,
  product,
  back: list,
}: {
  id: string;
  product?: string;
  /** The inventory list's crumb. */
  back: Crumb;
}) {
  const client = await sessionClient();
  const [item, locations, me, store, t, format, owner] = await Promise.all([
    client.GET("/admin/inventory-items/{id}", { params: { path: { id } } }).then(loaded),
    locationNames(client),
    requireStaff(),
    loadStore(),
    getT(),
    getFormat(),
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
  const title = found && variant ? found.title : t("inventory.detail_title_fallback");
  const back: Crumb[] =
    found && variant ? [list, { label: found.title, href: `/products/${found.id}` }] : [list];
  return (
    <>
      <PageHeader
        title={title}
        back={back}
        description={
          [variant?.title, variant?.sku, item.tracked ? null : t("inventory.not_tracked")]
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
        <MetadataList title={t("inventory.on_hand")}>
          {item.levels.length === 0 ? (
            <Text color="secondary">{t("inventory.detail_no_levels")}</Text>
          ) : (
            item.levels.map((level) => {
              const counts = {
                on_hand: level.on_hand,
                reserved: level.reserved,
                available: level.available,
              };
              const incoming = incomingLabel(level, t, format);
              return (
                <MetadataListItem
                  key={level.location}
                  label={locations.get(level.location) ?? t("inventory.removed_location")}
                >
                  <Text hasTabularNumbers>
                    {incoming
                      ? t("inventory.detail_level_counts_incoming", { ...counts, incoming })
                      : t("inventory.detail_level_counts", counts)}
                  </Text>
                </MetadataListItem>
              );
            })
          )}
        </MetadataList>
      </Card>
      {/* Absent without purchasing:read: cost is for those who buy. */}
      {item.unit_cost !== undefined && (
        <Card>
          <MetadataList title={t("inventory.detail_cost_title")}>
            <MetadataListItem label={t("inventory.unit_cost")}>
              <Text hasTabularNumbers>
                {item.unit_cost ? format.unitCost(item.unit_cost) : t("common.not_set")}
              </Text>
            </MetadataListItem>
          </MetadataList>
          {hasScope(me.scopes, "purchasing:write") &&
            (store?.currency ? (
              <SetCost item={id} cost={item.unit_cost} currency={store.currency} />
            ) : (
              <Text type="supporting" color="secondary">
                {t("inventory.detail_cost_needs_currency")}
              </Text>
            ))}
        </Card>
      )}
    </>
  );
}

async function History({ id, params }: { id: string; params: SearchParams }) {
  const client = await sessionClient();
  const [result, locations, me, t] = await Promise.all([
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
    getT(),
  ]);
  return (
    <VStack gap={3}>
      <Text type="display-3" as="h2">
        {t("inventory.history_title")}
      </Text>
      <HistoryTable
        page={await listPage(result, `/inventory/${id}`, params)}
        locations={Object.fromEntries(locations)}
        showCost={hasScope(me.scopes, "purchasing:read")}
      />
    </VStack>
  );
}
