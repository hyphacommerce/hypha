import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import {
  loadNotificationSettings,
  loadStore,
  loaded,
  requireStaff,
  sessionClient,
} from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";
import {
  isTranslated,
  templateDescription,
  templateLanguage,
  templateName,
} from "@/lib/notifications";

import { TemplateEditor } from "./template-editor";

const back = (t: T): Crumb[] => [
  settingsCrumb(t),
  { label: t("notifications.page_title"), href: "/settings/notifications" },
];

/**
 * One email's template in one of the store's languages (`?language=`; none: the content
 * language): its sources, variables and a live preview. The way back renders at once; the
 * title is the template's name, so the header streams in with the editor.
 */
export default async function TemplatePage({
  params,
  searchParams,
}: {
  params: Promise<{ key: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ key }, query, t] = await Promise.all([params, searchParams, getT()]);
  const asked = typeof query.language === "string" ? query.language : undefined;
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
          <Template templateKey={key} asked={asked} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Template({
  templateKey: key,
  asked,
}: {
  templateKey: string;
  asked: string | undefined;
}) {
  const client = await sessionClient();
  // The store's languages (the layout's read too: one call for both). A role that can't
  // read the store edits the content language's template alone.
  const store = await loadStore();
  const others = store?.languages.map((l) => l.language) ?? [];

  /** The template in `language` (`null`: the content language). */
  const templateIn = async (language: string | null) => ({
    language,
    saved: loaded(
      await client.GET("/admin/notification-templates/{key}", {
        params: { path: { key }, query: language ? { language } : {} },
      }),
    ),
  });

  // The first preview comes with the page, so it shows without waiting for an edit: it
  // renders the template's sources, so it follows their read (the rest go out with it).
  const [t, me, settings, { template, language, firstPreview }] = await Promise.all([
    getT(),
    requireStaff(),
    loadNotificationSettings(),
    templateIn(templateLanguage(asked, others))
      // Staff and supplier emails have one template: a link naming a language shows it.
      .then((found) =>
        found.language !== null && !isTranslated(found.saved.audience) ? templateIn(null) : found,
      )
      .then(async ({ saved, language: shown }) => {
        const preview = await client.POST("/admin/notification-templates/{key}/preview", {
          params: { path: { key } },
          body:
            key === "layout"
              ? { html: saved.html, language: shown }
              : { subject: saved.subject, html: saved.html, text: saved.text, language: shown },
        });
        return { template: saved, language: shown, firstPreview: preview.data ?? null };
      }),
  ]);
  const name = templateName(t, template.key);

  return (
    <>
      <PageHeader title={name} description={templateDescription(t, template.key)} back={back(t)} />
      <TemplateEditor
        // Another language is another template: a draft, preview and state of its own.
        key={language ?? ""}
        template={template}
        language={language}
        languages={
          store !== null && others.length > 0 && isTranslated(template.audience)
            ? { content: store.default_language, others: store.languages }
            : null
        }
        firstPreview={firstPreview}
        sender={settings?.sender ?? "log"}
        canWrite={me.scopes.includes("notifications:write")}
        canReadOrders={me.scopes.includes("orders:read")}
      />
    </>
  );
}
