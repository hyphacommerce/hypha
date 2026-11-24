import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { notFound } from "next/navigation";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { appName, loadExtensions } from "@/lib/app-extensions";
import { getT } from "@/lib/i18n";
import { appsCrumb } from "@/lib/navigation";

import { AppPageTabs } from "../../app-pages";
import { AppFrame } from "./app-frame";

/**
 * One of an app's embedded pages (`apps.page`, `settings.app`), at `?path=` within it
 * when the app navigated there. The way back renders at once; the page (named by the
 * apps' extensions) streams in behind a skeleton.
 */
export default async function AppEmbeddedPage({
  params,
  searchParams,
}: {
  params: Promise<{ key: string; id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ key, id }, query, t] = await Promise.all([params, searchParams, getT()]);
  const raw = typeof query.path === "string" ? query.path : "/";
  // Only a place within the app's page (docs/apps/bridge.md: one leading `/`).
  const path = raw.startsWith("/") && !raw.startsWith("//") ? raw : "/";

  return (
    <Section padding={6}>
      <VStack gap={4} maxWidth={1280}>
        <Suspense
          fallback={
            <>
              <PageHeader back={appsCrumb(t)} />
              <FormSkeleton cards={1} />
            </>
          }
        >
          <EmbeddedPage appKey={key} id={id} path={path} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function EmbeddedPage({
  appKey: key,
  id,
  path,
}: {
  appKey: string;
  id: string;
  path: string;
}) {
  const [extensions, t] = await Promise.all([loadExtensions(), getT()]);
  const pages = extensions.pages.filter((p) => p.app === key);
  const page = pages.find((p) => p.id === id);
  if (!page) notFound();
  const name = appName(extensions, key);

  return (
    <>
      <PageHeader title={page.title} back={[appsCrumb(t), { label: name, href: `/apps/${key}` }]} />
      <AppPageTabs t={t} appKey={key} pages={pages} current={page.id} />
      <AppFrame appKey={key} pageId={page.id} pageUrl={page.url} path={path} title={page.title} />
    </>
  );
}
