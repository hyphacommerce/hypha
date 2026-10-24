import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { DiscountStatusBadge } from "@/components/discounts/status-badge";
import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { FormSkeleton } from "@/components/skeletons";
import { type Discount, methodLabel, withDiscountMarkets } from "@/lib/discounts/wire";
import { loaded, perRequest, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { loadLive, loadNames, loadOptions } from "../data";
import { EditDiscount } from "../discount-editor";
import { CodesTable } from "./codes-table";
import { DeleteDiscount } from "./delete-discount";
import { type DiscountTab, DiscountTabs } from "./discount-tabs";
import { UsageTable } from "./usage-table";

const BACK: Crumb[] = [{ label: "Discounts", href: "/discounts" }];

/** The discounts this request read, by ID: the header and the tab share one read. */
const discountReads = perRequest(async () => new Map<string, Promise<Discount>>());

async function loadDiscount(id: string): Promise<Discount> {
  const reads = await discountReads();
  let read = reads.get(id);
  if (!read) {
    read = sessionClient().then((client) =>
      client.GET("/admin/discounts/{id}", { params: { path: { id } } }).then(loaded),
    );
    reads.set(id, read);
  }
  return read;
}

function tabOf(value: string | undefined): DiscountTab {
  return value === "codes" || value === "usage" ? value : "settings";
}

/** What a tab shows while its reads are on their way. */
function tabFallback(id: string, tab: DiscountTab) {
  if (tab === "codes") return <CodesTable id={id} canWrite={false} />;
  if (tab === "usage") return <UsageTable />;
  return <FormSkeleton cards={5} />;
}

/**
 * A discount: its settings (the form, saved from the save bar), its codes and the orders
 * it was used on, as tabs (`?tab=codes`, `?tab=usage`). The header and the tabs come
 * with the discount (an automatic one has no codes tab); each tab's own reads stream in
 * after.
 */
export default async function DiscountPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ id }, query] = await Promise.all([params, searchParams]);
  const tab = tabOf(one(query, "tab"));
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense
          fallback={
            <>
              <PageHeader back={BACK} />
              {tabFallback(id, tab)}
            </>
          }
        >
          <DiscountView id={id} tab={tab} query={query} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function DiscountView({
  id,
  tab: asked,
  query,
}: {
  id: string;
  tab: DiscountTab;
  query: SearchParams;
}) {
  const [discount, me] = await Promise.all([loadDiscount(id), requireStaff()]);
  const hasCodes = discount.method === "codes";
  // Usage names buyers and their orders, so it needs reading both.
  const hasUsage = hasScope(me.scopes, "customers:read") && hasScope(me.scopes, "orders:read");
  const tab =
    (asked === "codes" && !hasCodes) || (asked === "usage" && !hasUsage) ? "settings" : asked;
  return (
    <>
      <PageHeader
        title={discount.title}
        back={BACK}
        badges={<DiscountStatusBadge phase={discount.phase} />}
        description={`${methodLabel(discount)} · ${discount.summary}`}
      />
      <DiscountTabs id={id} tab={tab} hasCodes={hasCodes} hasUsage={hasUsage}>
        <Suspense fallback={tabFallback(id, tab)}>
          {tab === "codes" ? (
            <Codes id={id} params={query} />
          ) : tab === "usage" ? (
            <Usage id={id} params={query} discount={discount} />
          ) : (
            <Settings id={id} />
          )}
        </Suspense>
      </DiscountTabs>
    </>
  );
}

async function Settings({ id }: { id: string }) {
  const client = await sessionClient();
  const [discount, me, options, live] = await Promise.all([
    loadDiscount(id),
    requireStaff(),
    loadOptions(client),
    loadLive(client),
  ]);
  const names = await loadNames(client, discount);
  const canWrite = hasScope(me.scopes, "discounts:write");
  // From the server, so the browser judges the summary's phase at the same instant. A
  // server component renders once per request, so reading the clock here is fine.
  // oxlint-disable-next-line react/purity
  const now = Date.now();
  return (
    <VStack gap={5}>
      <EditDiscount
        key={discount.updated_at}
        discount={discount}
        options={{ ...options, markets: withDiscountMarkets(options.markets, discount) }}
        names={names}
        live={live}
        now={now}
        isReadOnly={!canWrite}
      />
      {canWrite && <DeleteDiscount id={discount.id} title={discount.title} used={discount.used} />}
    </VStack>
  );
}

async function Codes({ id, params }: { id: string; params: SearchParams }) {
  const client = await sessionClient();
  const active = one(params, "active");
  const [me, result] = await Promise.all([
    requireStaff(),
    client.GET("/admin/discounts/{id}/codes", {
      params: {
        path: { id },
        query: {
          ...pageQuery(params),
          q: one(params, "q")?.trim() || undefined,
          batch: one(params, "batch")?.trim() || undefined,
          active: active === "true" ? true : active === "false" ? false : undefined,
        },
      },
    }),
  ]);
  return (
    <CodesTable
      id={id}
      page={listPage(result, `/discounts/${id}`, params)}
      canWrite={hasScope(me.scopes, "discounts:write")}
      batch={one(params, "batch")?.trim() || null}
    />
  );
}

async function Usage({
  id,
  params,
  discount,
}: {
  id: string;
  params: SearchParams;
  discount: Discount;
}) {
  const client = await sessionClient();
  const result = await client.GET("/admin/discounts/{id}/usage", {
    params: { path: { id }, query: pageQuery(params) },
  });
  return (
    <UsageTable
      page={listPage(result, `/discounts/${id}`, params)}
      used={discount.used}
      totals={discount.discounted_total}
    />
  );
}
