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
import { getFormat } from "@/lib/get-format";
import { loaded, perRequest, requireStaff, sessionClient } from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { hasScope } from "@/lib/navigation";
import {
  MAX_LISTS_PER_CATALOG,
  type PriceList,
  adjustmentLabel,
  kindLabel,
  phaseBadge,
} from "@/lib/price-lists";

import { pricesHref } from "@/lib/price-editor";

import { productSummaries } from "../grid-data";
import { EditPriceListForm } from "../price-list-form";
import { ListWindow } from "../price-lists-table";
import { DeletePriceList } from "./delete-list";
import { ListTabs, type ListTab } from "./list-tabs";
import { WhereCard } from "./where-card";

/** The way back: the markets, then the price lists. */
function backOf(t: T): Crumb[] {
  return [
    { label: t("common.markets"), href: "/markets" },
    { label: t("price_lists.title"), href: "/markets/price-lists" },
  ];
}

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
  const [{ id }, query, t] = await Promise.all([params, searchParams, getT()]);
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
        <Suspense fallback={<PageHeader back={backOf(t)} />}>
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
  const [list, t, format] = await Promise.all([loadList(id), getT(), getFormat()]);
  const phase = phaseBadge(list.phase, t);
  return (
    <>
      <PageHeader
        title={list.name}
        back={backOf(t)}
        badges={
          <>
            <Badge
              label={kindLabel(t, list.kind)}
              variant={list.kind === "sale" ? "pink" : "blue"}
            />
            <Badge label={phase.label} variant={phase.variant} />
          </>
        }
        description={[
          list.currency,
          adjustmentLabel(t, format, list.adjustment),
          t("price_lists.fixed_prices_count", { count: list.fixed_price_count }),
        ].join(" · ")}
      />
      {attachFailed && (
        <Banner
          status="warning"
          title={t("price_lists.detail_attach_failed_title")}
          description={t("price_lists.detail_attach_failed_description", {
            max: MAX_LISTS_PER_CATALOG,
          })}
        />
      )}
    </>
  );
}

async function Settings({ id }: { id: string }) {
  const client = await sessionClient();
  const [t, format, list, me, markets, categories, groups] = await Promise.all([
    getT(),
    getFormat(),
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
          <MetadataList title={t("price_lists.detail_about")}>
            <MetadataListItem label={t("price_lists.detail_when")}>
              <ListWindow list={list} />
            </MetadataListItem>
            <MetadataListItem label={t("price_lists.detail_fixed_prices")}>
              <Text>{format.number(list.fixed_price_count)}</Text>
            </MetadataListItem>
            <MetadataListItem label={t("price_lists.detail_id")}>
              <Text type="supporting" color="secondary">
                {list.id}
              </Text>
            </MetadataListItem>
            <MetadataListItem label={t("common.created")}>
              <Text type="supporting">
                <DateTime value={list.created_at} />
              </Text>
            </MetadataListItem>
            <MetadataListItem label={t("price_lists.detail_last_changed")}>
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
  const [list, me, t] = await Promise.all([loadList(id), requireStaff(), getT()]);
  const canWrite = hasScope(me.scopes, "pricing:write");
  const count = list.fixed_price_count;
  return (
    <Card>
      <VStack gap={4} hAlign="start">
        <VStack gap={1}>
          <Text weight="medium">
            {count === 0
              ? t("price_lists.detail_prices_none")
              : t("price_lists.detail_prices_count", { count, currency: list.currency })}
          </Text>
          <Text type="supporting" color="secondary">
            {canWrite
              ? t("price_lists.detail_editor_hint_write")
              : t("price_lists.detail_editor_hint_read")}
          </Text>
        </VStack>
        <Button
          label={
            canWrite ? t("price_lists.detail_open_editor") : t("price_lists.detail_view_prices")
          }
          variant="primary"
          icon={<TableCellsIcon width={16} height={16} />}
          href={pricesHref(id, { fixedOnly: false })}
        />
      </VStack>
    </Card>
  );
}
