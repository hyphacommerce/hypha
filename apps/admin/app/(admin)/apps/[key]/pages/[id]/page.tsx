import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { notFound } from "next/navigation";

import { PageHeader } from "@/components/page-header";
import { appName, loadExtensions } from "@/lib/app-extensions";

import { AppPageTabs } from "../../app-pages";
import { AppFrame } from "./app-frame";

/**
 * One of an app's embedded pages (`apps.page`, `settings.app`), at `?path=` within it
 * when the app navigated there.
 */
export default async function AppEmbeddedPage({
  params,
  searchParams,
}: {
  params: Promise<{ key: string; id: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ key, id }, query, extensions] = await Promise.all([
    params,
    searchParams,
    loadExtensions(),
  ]);
  const pages = extensions.pages.filter((p) => p.app === key);
  const page = pages.find((p) => p.id === id);
  if (!page) notFound();
  const raw = typeof query.path === "string" ? query.path : "/";
  // Only a place within the app's page (docs/apps/bridge.md: one leading `/`).
  const path = raw.startsWith("/") && !raw.startsWith("//") ? raw : "/";
  const name = appName(extensions, key);

  return (
    <Section padding={6}>
      <VStack gap={4} maxWidth={1280}>
        <PageHeader
          title={page.title}
          back={[
            { label: "Apps", href: "/apps" },
            { label: name, href: `/apps/${key}` },
          ]}
        />
        <AppPageTabs appKey={key} pages={pages} current={page.id} />
        <AppFrame appKey={key} pageId={page.id} pageUrl={page.url} path={path} title={page.title} />
      </VStack>
    </Section>
  );
}
