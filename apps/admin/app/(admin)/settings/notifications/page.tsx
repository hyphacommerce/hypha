import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loadNotificationSettings, requireStaff, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { SenderBanner } from "./sender-banner";
import { NotificationSettingsForm } from "./settings-form";
import { TemplatesCard } from "./templates";

/** Settings → Notifications: how the store's emails look, who gets the staff ones, and each
 * email's template. The header renders at once; the settings and templates stream in. */
export default function NotificationsSettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Notifications"
          description="The emails your store sends: order confirmations and shipping updates to customers, new orders to staff, and staff account emails."
        />
        <Suspense fallback={<FormSkeleton cards={5} />}>
          <Notifications />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Notifications() {
  const client = await sessionClient();
  const [me, settings, templates] = await Promise.all([
    requireStaff(),
    loadNotificationSettings(),
    client.GET("/admin/notification-templates"),
  ]);
  if (settings === null) {
    return (
      <Banner
        status="info"
        title="Your role can't see notifications"
        description="Ask the store owner for the notifications:read scope."
      />
    );
  }
  return (
    <>
      <SenderBanner sender={settings.sender} from={settings.from} />
      <NotificationSettingsForm
        settings={settings}
        canWrite={me.scopes.includes("notifications:write")}
      />
      <TemplatesCard
        templates={templates.data?.items ?? []}
        canWrite={me.scopes.includes("notifications:write")}
      />
    </>
  );
}
