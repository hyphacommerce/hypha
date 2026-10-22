import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { takenCountries } from "@/lib/regions";

import { NewMarketForm } from "./new-market-form";

/** The header renders at once; the form waits for the countries and the markets taken. */
export default function NewMarketPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          title="Add market"
          description="Countries that buy in one currency. Its catalog is created with it; add products to it next."
          back={{ label: "Markets", href: "/markets" }}
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
