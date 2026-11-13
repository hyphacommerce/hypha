import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { getFormat } from "@/lib/get-format";
import { loadCountries, loaded, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { countryLabel } from "@/lib/regions";
import { MarketsTable } from "@/resources/markets";

/**
 * Markets: who the store sells to, in which currency. The header renders at once; the
 * markets stream in.
 */
export default async function MarketsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title={t("common.markets")}
          description={t("markets.list_description")}
          actions={<Button label={t("markets.add_market")} variant="primary" href="/markets/new" />}
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
  const [markets, countries, format] = await Promise.all([
    client.GET("/admin/markets").then(loaded),
    loadCountries(),
    getFormat(),
  ]);
  return (
    <MarketsTable
      markets={markets.items.map((m) => ({
        ...m,
        countryNames: format.list(
          m.countries.map((c) => countryLabel(c, countries, format.locale)),
          "unit",
        ),
      }))}
    />
  );
}
