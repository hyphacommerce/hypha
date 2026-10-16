import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loadCountries } from "@/lib/hypha";

import { NewMarketForm } from "./new-market-form";

export default async function NewMarketPage() {
  const countries = await loadCountries();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          title="Add market"
          description="Countries that buy in one currency. Its catalog is created with it; add products to it next."
          back={{ label: "Markets", href: "/markets" }}
        />
        <NewMarketForm countries={countries} />
      </VStack>
    </Section>
  );
}
