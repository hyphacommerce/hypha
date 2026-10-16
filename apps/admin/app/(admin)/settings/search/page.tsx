import { Section } from "@astryxdesign/core/Section";
import { VStack } from "@astryxdesign/core/Layout";

import { PageHeader } from "@/components/page-header";
import { loaded, sessionClient } from "@/lib/hypha";

import { IndexStatus } from "./index-status";
import { SearchSettingsForm } from "./search-settings-form";

/**
 * How the storefront's search and product listings work (design note 0019): which
 * options split listings, which filters shoppers see, and the index behind them.
 */
export default async function SearchSettingsPage() {
  const client = await sessionClient();
  const [settings, status] = await Promise.all([
    client.GET("/admin/search/settings").then(loaded),
    client.GET("/admin/search/status").then(loaded),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={720}>
        <PageHeader
          title="Search"
          description="How shoppers find products: listings, filters and the search index."
        />
        <SearchSettingsForm key={JSON.stringify(settings)} settings={settings} />
        <IndexStatus status={status} />
      </VStack>
    </Section>
  );
}
