import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { loaded, sessionClient } from "@/lib/hypha";

import { StoreForm } from "./store-form";

export default async function StoreSettingsPage() {
  const client = await sessionClient();
  const store = loaded(await client.GET("/admin/store"));

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={640}>
        <PageHeader title="Store" description="The store this deployment serves." />
        <Card>
          <StoreForm name={store.name} />
        </Card>
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
