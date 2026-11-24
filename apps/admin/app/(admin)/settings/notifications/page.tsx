import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loadNotificationSettings, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { SenderBanner } from "./sender-banner";
import { NotificationSettingsForm } from "./settings-form";
import { TemplatesCard } from "./templates";

/** Settings → Notifications: how the store's emails look, who gets the staff ones, and each
 * email's template. The header renders at once; the settings and templates stream in. */
export default async function NotificationsSettingsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("notifications.page_title")}
          description={t("notifications.page_description")}
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
  const [t, me, settings, templates] = await Promise.all([
    getT(),
    requireStaff(),
    loadNotificationSettings(),
    client.GET("/admin/notification-templates"),
  ]);
  if (settings === null) {
    return (
      <Banner
        status="info"
        title={t("notifications.no_access_title")}
        description={t("notifications.no_access_description")}
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
