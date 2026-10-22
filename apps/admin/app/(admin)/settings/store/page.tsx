import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_CRUMB } from "@/lib/navigation";
import { loadCountries, requireStore } from "@/lib/hypha";

import { SetupGuideCard } from "./setup-guide-card";
import { StoreForm } from "./store-form";

/** Settings → Store: the store's name and details. */
export default async function StoreSettingsPage() {
  // The store is the layout's read too (`loadStore`): one call for both.
  const [store, countries] = await Promise.all([requireStore(), loadCountries()]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Store details"
          description="The store this deployment serves."
        />
        <StoreForm store={store} countries={countries} />
        {store.setup.finished_at && <SetupGuideCard />}
        <Card>
          <MetadataList title="Details">
            <MetadataListItem label="ID">
              <Text type="supporting" color="secondary">
                {store.id}
              </Text>
            </MetadataListItem>
          </MetadataList>
        </Card>
      </VStack>
    </Section>
  );
}
