import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { loadSearchCatalog } from "./catalog";
import { IndexStatus } from "./index-status";
import { SearchSettingsForm } from "./search-settings-form";

/**
 * How the storefront's search and product listings work: which filters shoppers see
 * and which options split listings, offered from what the store's products actually
 * have, beside the index behind them. The header renders at once; the form streams in.
 */
export default function SearchSettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Search"
          description="The filters and listings shoppers get on your storefront's search and category pages."
        />
        <Suspense fallback={<FormSkeleton cards={4} />}>
          <SearchSettings />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function SearchSettings() {
  const client = await sessionClient();
  const [settings, status, catalog] = await Promise.all([
    client.GET("/admin/search/settings").then(loaded),
    client.GET("/admin/search/status").then(loaded),
    loadSearchCatalog(client),
  ]);
  return (
    <SearchSettingsForm
      key={JSON.stringify(settings)}
      settings={settings}
      catalog={catalog}
      status={<IndexStatus key={JSON.stringify(status)} status={status} />}
    />
  );
}
