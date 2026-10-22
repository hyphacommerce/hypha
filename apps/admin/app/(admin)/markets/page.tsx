import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { countryLabel } from "@/lib/regions";
import { MarketsTable } from "@/resources/markets";

/**
 * Markets: who the store sells to, in which currency. The header renders at once; the
 * markets stream in.
 */
export default function MarketsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Markets"
          description="Countries that buy in one currency, with the catalog of what they can buy and its prices."
          actions={<Button label="Add market" variant="primary" href="/markets/new" />}
        />
        <Suspense fallback={<TableSkeleton rows={4} />}>
          <MarketRows />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function MarketRows() {
  const client = await sessionClient();
  const [markets, countries] = await Promise.all([
    client.GET("/admin/markets").then(loaded),
    loadCountries(),
  ]);
  return (
    <MarketsTable
      markets={markets.items.map((m) => ({
        ...m,
        countryNames: m.countries.map((c) => countryLabel(c, countries)).join(", "),
      }))}
    />
  );
}
