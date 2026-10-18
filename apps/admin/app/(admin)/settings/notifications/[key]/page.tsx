import { Section } from "@astryxdesign/core/Section";
import { VStack } from "@astryxdesign/core/Layout";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_CRUMB } from "@/lib/navigation";
import { loadNotificationSettings, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { TEMPLATES, templateName } from "@/lib/notifications";

import { TemplateEditor } from "./template-editor";

/** One email's template (design note 0028, §7): its sources, variables and a live
 * preview. */
export default async function TemplatePage({ params }: { params: Promise<{ key: string }> }) {
  const { key } = await params;
  const client = await sessionClient();
  // The first preview comes with the page, so it shows without waiting for an edit: it
  // renders the template's sources, so it follows their read (the rest go out with it).
  const [me, settings, { template, firstPreview }] = await Promise.all([
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
  const name = templateName(template.key);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1440}>
        <PageHeader
          title={name}
          description={TEMPLATES[template.key]?.description}
          back={[SETTINGS_CRUMB, { label: "Notifications", href: "/settings/notifications" }]}
        />
        <TemplateEditor
          template={template}
          firstPreview={firstPreview}
          sender={settings?.sender ?? "log"}
          canWrite={me.scopes.includes("notifications:write")}
          canReadOrders={me.scopes.includes("orders:read")}
        />
      </VStack>
    </Section>
  );
}
