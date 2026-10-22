import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_CRUMB } from "@/lib/navigation";
import { loaded, sessionClient } from "@/lib/hypha";

import { AppsCard } from "./apps";

/** Settings → Apps: external apps, their signing secrets, and the events they get as
 * webhooks. */
export default async function AppsSettingsPage() {
  const client = await sessionClient();
  const apps = await client.GET("/admin/apps").then(loaded);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Webhooks and custom apps"
          description="Services Hypha calls, signed with each app's secret: webhooks for the events they subscribe to, and live shipping rates."
        />
        <AppsCard apps={apps.items} />
      </VStack>
    </Section>
  );
}
