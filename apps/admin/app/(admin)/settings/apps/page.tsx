import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { AppsCard } from "./apps";

/** Settings → Apps: external apps, their signing secrets, and the events they get as
 * webhooks. The header renders at once; the apps stream in. */
export default function AppsSettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Webhooks and custom apps"
          description="Services Hypha calls, signed with each app's secret: webhooks for the events they subscribe to, and live shipping rates."
        />
        <Suspense fallback={<CardSkeleton lines={4} />}>
          <Apps />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Apps() {
  const client = await sessionClient();
  const apps = await client.GET("/admin/apps").then(loaded);
  return <AppsCard apps={apps.items} />;
}
