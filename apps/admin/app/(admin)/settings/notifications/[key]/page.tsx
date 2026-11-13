import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { loadNotificationSettings, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";
import { templateDescription, templateName } from "@/lib/notifications";

import { TemplateEditor } from "./template-editor";

const back = (t: T): Crumb[] => [
  settingsCrumb(t),
  { label: t("notifications.page_title"), href: "/settings/notifications" },
];

/**
 * One email's template: its sources, variables and a live preview. The way back renders
 * at once; the title is the template's name, so the header streams in with the editor.
 */
export default async function TemplatePage({ params }: { params: Promise<{ key: string }> }) {
  const [{ key }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1440}>
        <Suspense
          fallback={
            <>
              <PageHeader back={back(t)} />
              <FormSkeleton cards={2} />
            </>
          }
        >
          <Template templateKey={key} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Template({ templateKey: key }: { templateKey: string }) {
  const client = await sessionClient();
  // The first preview comes with the page, so it shows without waiting for an edit: it
  // renders the template's sources, so it follows their read (the rest go out with it).
  const [t, me, settings, { template, firstPreview }] = await Promise.all([
    getT(),
    requireStaff(),
    loadNotificationSettings(),
    client
      .GET("/admin/notification-templates/{key}", { params: { path: { key } } })
      .then(async (found) => {
        const saved = loaded(found);
        const preview = await client.POST("/admin/notification-templates/{key}/preview", {
          params: { path: { key } },
          body:
            key === "layout"
              ? { html: saved.html }
              : { subject: saved.subject, html: saved.html, text: saved.text },
        });
        return { template: saved, firstPreview: preview.data ?? null };
      }),
  ]);
  const name = templateName(t, template.key);

  return (
    <>
      <PageHeader title={name} description={templateDescription(t, template.key)} back={back(t)} />
      <TemplateEditor
        template={template}
        firstPreview={firstPreview}
        sender={settings?.sender ?? "log"}
        canWrite={me.scopes.includes("notifications:write")}
        canReadOrders={me.scopes.includes("orders:read")}
      />
    </>
  );
}
