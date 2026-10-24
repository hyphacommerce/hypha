import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { TableCellsIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { CardSkeleton, DetailSkeleton } from "@/components/skeletons";
import { flatten } from "@/lib/categories";
import { loaded, perRequest, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";
import { type PriceList, adjustmentLabel, kindLabel, phaseBadge } from "@/lib/price-lists";

import { pricesHref } from "@/lib/price-editor";

import { productSummaries } from "../grid-data";
import { EditPriceListForm } from "../price-list-form";
import { ListWindow } from "../price-lists-table";
import { DeletePriceList } from "./delete-list";
import { ListTabs, type ListTab } from "./list-tabs";
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

/**
 * A price list: its settings and the markets using it (`?tab=` absent), or what its
 * prices tab says and the way into the full-window price editor (`?tab=prices`). The
 * way back and the tabs render at once; the list streams in.
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
  // The grid used to live in this tab: its old addresses open the editor where they were.
  if (tab === "prices" && ["q", "category", "fixed", "cursor"].some((key) => one(query, key))) {
    redirect(
      pricesHref(
        id,
        {
          q: one(query, "q"),
          category: one(query, "category"),
          fixedOnly: one(query, "fixed") === "1",
        },
        one(query, "cursor"),
      ),
    );
  }
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense fallback={<PageHeader back={BACK} />}>
          <ListHeader id={id} attachFailed={one(query, "attach") === "failed"} />
        </Suspense>
        <ListTabs id={id} tab={tab}>
          {tab === "prices" ? (
            <Suspense fallback={<CardSkeleton lines={2} />}>
              <Prices id={id} />
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
  const [list, me, markets, categories, groups] = await Promise.all([
    loadList(id),
    requireStaff(),
    client.GET("/admin/markets"),
    client.GET("/admin/categories"),
    // Without `customers:read`, none: the audience shows by ID.
    client.GET("/admin/customer-groups"),
  ]);
  const to = list.adjustment?.applies_to;
  // The adjustment names products by ID: their titles for the chips (a product gone, or
  // a failed read, shows by ID).
  const named = to?.type === "products" ? to.products : [];
  const titles = await productSummaries(client, named).then(
    (products) => products.map((p) => [p.id, p.title] as const),
    () => [],
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
          groups={(groups.data?.items ?? []).map((g) => ({ value: g.id, label: g.name }))}
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

/**
 * The prices tab: what the list holds and the way into the editor, which takes the whole
 * window (`/markets/price-lists/{id}/prices`).
 */
async function Prices({ id }: { id: string }) {
  const [list, me] = await Promise.all([loadList(id), requireStaff()]);
  const canWrite = hasScope(me.scopes, "pricing:write");
  const count = list.fixed_price_count;
  return (
    <Card>
      <VStack gap={4} hAlign="start">
        <VStack gap={1}>
          <Text weight="medium">
            {count === 0
              ? "No fixed prices yet"
              : `${count.toLocaleString("en")} fixed ${count === 1 ? "price" : "prices"} in ${list.currency}`}
          </Text>
          <Text type="supporting" color="secondary">
            {canWrite
              ? "The price editor shows every variant with its base price and the price this list gives it. Click a cell and type to set a fixed price, paste a column from a spreadsheet, or import a CSV."
              : "The price editor shows every variant with its base price and the price this list gives it."}
          </Text>
        </VStack>
        <Button
          label={canWrite ? "Open price editor" : "View prices"}
          variant="primary"
          icon={<TableCellsIcon width={16} height={16} />}
          href={pricesHref(id, { fixedOnly: false })}
        />
      </VStack>
    </Card>
  );
}
