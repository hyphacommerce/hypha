import { Section } from "@astryxdesign/core/Section";
import { VStack } from "@astryxdesign/core/Layout";

import { PageHeader } from "@/components/page-header";
import { loaded, sessionClient } from "@/lib/hypha";

import { loadSearchCatalog } from "./catalog";
import { IndexStatus } from "./index-status";
import { SearchSettingsForm } from "./search-settings-form";

/**
 * How the storefront's search and product listings work (design notes 0019 and 0024):
 * which filters shoppers see and which options split listings, offered from what the
 * store's products actually have, beside the index behind them.
 */
export default async function SearchSettingsPage() {
  const client = await sessionClient();
  const [settings, status, catalog] = await Promise.all([
    client.GET("/admin/search/settings").then(loaded),
    client.GET("/admin/search/status").then(loaded),
    loadSearchCatalog(client),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <PageHeader
          title="Search"
          description="The filters and listings shoppers get on your storefront's search and category pages."
        />
        <SearchSettingsForm
          key={JSON.stringify(settings)}
          settings={settings}
          catalog={catalog}
          status={<IndexStatus key={JSON.stringify(status)} status={status} />}
        />
      </VStack>
    </Section>
  );
}
