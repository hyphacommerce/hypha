import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { redirect } from "next/navigation";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { DetailSkeleton, TableSkeleton } from "@/components/skeletons";
import { flatten } from "@/lib/categories";
import { describe, loaded, perRequest, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { type PriceList, adjustmentLabel, kindLabel, phaseBadge } from "@/lib/price-lists";

import { type GridFilters, gridPage } from "../grid-data";
import { EditPriceListForm } from "../price-list-form";
import { ListWindow } from "../price-lists-table";
import { DeletePriceList } from "./delete-list";
import { ListTabs, type ListTab } from "./list-tabs";
import { PricesEditor } from "./prices-editor";
import { WhereCard } from "./where-card";

const BACK: Crumb[] = [
  { label: "Markets", href: "/markets" },
  { label: "Price lists", href: "/markets/price-lists" },
];

/** The lists this request read, by ID: the header and the tab share one read. */
const listReads = perRequest(async () => new Map<string, Promise<PriceList>>());

async function loadList(id: string): Promise<PriceList> {
  const reads = await listReads();
  let read = reads.get(id);
  if (!read) {
    read = sessionClient().then((client) =>
      client.GET("/admin/price-lists/{id}", { params: { path: { id } } }).then(loaded),
    );
    reads.set(id, read);
  }
  return read;
}

/** The prices tab's filters, from the URL. */
function filtersOf(params: SearchParams): GridFilters {
  return {
    q: one(params, "q"),
    category: one(params, "category"),
    fixedOnly: one(params, "fixed") === "1",
  };
}

/**
 * A price list: its settings and the markets using it (`?tab=` absent), or its prices
 * as a grid (`?tab=prices`, paged by `cursor`, narrowed by `q`, `category` and
 * `fixed=1`). The way back and the tabs render at once; the list streams in.
 */
export default async function PriceListPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ id }, query] = await Promise.all([params, searchParams]);
  const tab: ListTab = one(query, "tab") === "prices" ? "prices" : "settings";
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense fallback={<PageHeader back={BACK} />}>
          <ListHeader id={id} attachFailed={one(query, "attach") === "failed"} />
        </Suspense>
        <ListTabs id={id} tab={tab}>
          {tab === "prices" ? (
            <Suspense fallback={<TableSkeleton rows={10} />}>
              <Prices id={id} filters={filtersOf(query)} cursor={one(query, "cursor")} />
            </Suspense>
          ) : (
            <Suspense fallback={<DetailSkeleton />}>
              <Settings id={id} />
            </Suspense>
          )}
        </ListTabs>
      </VStack>
    </Section>
  );
}

async function ListHeader({ id, attachFailed }: { id: string; attachFailed: boolean }) {
  const list = await loadList(id);
  const phase = phaseBadge(list.phase);
  return (
    <>
      <PageHeader
        title={list.name}
        back={BACK}
        badges={
          <>
            <Badge label={kindLabel(list.kind)} variant={list.kind === "sale" ? "pink" : "blue"} />
            <Badge label={phase.label} variant={phase.variant} />
          </>
        }
        description={`${list.currency} · ${adjustmentLabel(list.adjustment)} · ${list.fixed_price_count.toLocaleString("en")} fixed ${list.fixed_price_count === 1 ? "price" : "prices"}`}
      />
      {attachFailed && (
        <Banner
          status="warning"
          title="The list was created, but not every market took it"
          description="A market's catalog holds at most 50 lists, all in its currency. Add it to the markets missing under Markets below."
        />
      )}
    </>
  );
}

async function Settings({ id }: { id: string }) {
  const client = await sessionClient();
  const [list, me, markets, categories] = await Promise.all([
    loadList(id),
    requireStaff(),
    client.GET("/admin/markets"),
    client.GET("/admin/categories"),
  ]);
  const to = list.adjustment?.applies_to;
  // The adjustment names products by ID: read the first ones' titles for the chips.
  const named = to?.type === "products" ? to.products.slice(0, 100) : [];
  const titles = await Promise.all(
    named.map((product) =>
      client
        .GET("/admin/products/{id}", { params: { path: { id: product } } })
        .then((r) => [product, r.data?.title ?? product] as const),
    ),
  );
  const canWrite = hasScope(me.scopes, "pricing:write");

  return (
    <div className="detail-columns">
      <div>
        <EditPriceListForm
          key={list.updated_at}
          list={list}
          categories={flatten(categories.data?.items ?? []).map((row) => ({
            value: row.id,
            label: row.path,
          }))}
          productNames={Object.fromEntries(titles)}
          isReadOnly={!canWrite}
        />
      </div>
      <div>
        <WhereCard
          list={list}
          markets={(markets.data?.items ?? []).flatMap((m) =>
            m.catalog ? [{ id: m.id, name: m.name, currency: m.currency, catalog: m.catalog }] : [],
          )}
          canWrite={hasScope(me.scopes, "markets:write")}
        />
        <Card>
          <MetadataList title="About">
            <MetadataListItem label="When">
              <ListWindow list={list} />
            </MetadataListItem>
            <MetadataListItem label="Fixed prices">
              <Text>{list.fixed_price_count.toLocaleString("en")}</Text>
            </MetadataListItem>
            <MetadataListItem label="Price list ID">
              <Text type="supporting" color="secondary">
                {list.id}
              </Text>
            </MetadataListItem>
            <MetadataListItem label="Created">
              <Text type="supporting">
                <DateTime value={list.created_at} />
              </Text>
            </MetadataListItem>
            <MetadataListItem label="Last changed">
              <Text type="supporting">
                <DateTime value={list.updated_at} />
              </Text>
            </MetadataListItem>
          </MetadataList>
        </Card>
        {canWrite && <DeletePriceList id={list.id} name={list.name} />}
      </div>
    </div>
  );
}

async function Prices({
  id,
  filters,
  cursor,
}: {
  id: string;
  filters: GridFilters;
  cursor: string | undefined;
}) {
  const client = await sessionClient();
  const [list, me, categories] = await Promise.all([
    loadList(id),
    requireStaff(),
    client.GET("/admin/categories"),
  ]);
  const tree = categories.data?.items ?? [];
  const page = await gridPage(client, list, tree, filters, cursor);
  const base = `/markets/price-lists/${id}`;
  const query = {
    tab: "prices",
    q: filters.q,
    category: filters.category,
    fixed: filters.fixedOnly ? "1" : undefined,
  };
  if (page.failure?.status === 400) {
    // An old or foreign cursor (a hand-edited URL): start the pages again.
    const search = new URLSearchParams(
      Object.entries(query).filter((e): e is [string, string] => Boolean(e[1])),
    );
    redirect(cursor ? `${base}?${search}` : `${base}?tab=prices`);
  }
  return (
    <PricesEditor
      pageKey={`${filters.q ?? ""}|${filters.category ?? ""}|${filters.fixedOnly}|${cursor ?? ""}`}
      list={list}
      products={page.products}
      nextCursor={page.nextCursor}
      isFirstPage={!cursor}
      filters={filters}
      error={page.failure?.error ? describe(page.failure.error) : undefined}
      categories={flatten(tree).map((row) => ({ value: row.id, label: row.path }))}
      canWrite={hasScope(me.scopes, "pricing:write")}
    />
  );
}
