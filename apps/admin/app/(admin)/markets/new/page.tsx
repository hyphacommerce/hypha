import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { takenCountries } from "@/lib/regions";

import { NewMarketForm } from "./new-market-form";

export default async function NewMarketPage() {
  const client = await sessionClient();
  const [countries, markets] = await Promise.all([
    loadCountries(),
    client.GET("/admin/markets").then(loaded),
  ]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          title="Add market"
          description="Countries that buy in one currency. Its catalog is created with it; add products to it next."
          back={{ label: "Markets", href: "/markets" }}
        />
        <NewMarketForm countries={countries} taken={takenCountries(markets.items)} />
      </VStack>
    </Section>
  );
}
