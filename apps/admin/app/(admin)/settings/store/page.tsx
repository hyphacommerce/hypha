import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loadCountries, requireStore } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { SetupGuideCard } from "./setup-guide-card";
import { StoreForm } from "./store-form";

/** Settings → Store: the store's name and details. */
export default async function StoreSettingsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("settings.store_title")}
          description={t("settings.store_description")}
        />
        <Suspense fallback={<FormSkeleton />}>
          <StoreDetails />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function StoreDetails() {
  // The store is the layout's read too (`loadStore`): one call for both.
  const [t, store, countries] = await Promise.all([getT(), requireStore(), loadCountries()]);
  return (
    <>
      <StoreForm store={store} countries={countries} />
      {store.setup.finished_at && <SetupGuideCard />}
      <Card>
        <MetadataList title={t("common.details")}>
          <MetadataListItem label={t("common.id")}>
            <Text type="supporting" color="secondary">
              {store.id}
            </Text>
          </MetadataListItem>
        </MetadataList>
      </Card>
    </>
  );
}
