import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { takenCountries } from "@/lib/regions";

import { NewMarketForm } from "./new-market-form";

/** The header renders at once; the form waits for the countries and the markets taken. */
export default async function NewMarketPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          title={t("markets.new_title")}
          description={t("markets.new_description")}
          back={{ label: t("common.markets"), href: "/markets" }}
        />
        <Suspense fallback={<FormSkeleton />}>
          <MarketForm />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function MarketForm() {
  const client = await sessionClient();
  const [countries, markets] = await Promise.all([
    loadCountries(),
    client.GET("/admin/markets").then(loaded),
  ]);
  return <NewMarketForm countries={countries} taken={takenCountries(markets.items)} />;
}
