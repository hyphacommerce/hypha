import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { FormSkeleton } from "@/components/skeletons";
import { flatten } from "@/lib/categories";
import { sessionClient } from "@/lib/hypha";

import { type MarketOption, NewPriceListForm } from "../price-list-form";

const BACK = [
  { label: "Markets", href: "/markets" },
  { label: "Price lists", href: "/markets/price-lists" },
];

/**
 * A new price list, from a template. `?market=` (the market page's "New price list")
 * puts it on that market and starts in its currency. The header renders at once; the
 * form waits for the markets and categories.
 */
export default async function NewPriceListPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={880}>
        <PageHeader
          title="New price list"
          description="Prices that differ from the base prices, for the markets that use the list."
          back={BACK}
        />
        <Suspense fallback={<FormSkeleton cards={4} />}>
          <Form market={one(params, "market")} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Form({ market }: { market: string | undefined }) {
  const client = await sessionClient();
  const [markets, categories] = await Promise.all([
    client.GET("/admin/markets"),
    client.GET("/admin/categories"),
  ]);
  const options: MarketOption[] = (markets.data?.items ?? []).flatMap((m) =>
    m.catalog ? [{ id: m.id, name: m.name, currency: m.currency, catalog: m.catalog }] : [],
  );
  const preselected = options.find((m) => m.id === market);
  return (
    <NewPriceListForm
      defaultCurrency={preselected?.currency ?? options[0]?.currency ?? "EUR"}
      markets={options}
      preselected={preselected?.id}
      categories={flatten(categories.data?.items ?? []).map((row) => ({
        value: row.id,
        label: row.path,
      }))}
    />
  );
}
