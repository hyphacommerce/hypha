import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";

import type { PageExtension } from "@/lib/app-extensions";
import type { TFor } from "@/lib/messages";

/** An app's own pages (`apps.page`, `settings.app`), as links between them. */
export function AppPageTabs({
  t,
  appKey,
  pages,
  current,
}: {
  t: TFor<"apps">;
  appKey: string;
  pages: PageExtension[];
  /** The page shown, or none on the app's overview. */
  current?: string;
}) {
  if (pages.length === 0) return null;
  return (
    <HStack gap={2} wrap="wrap">
      <Button
        label={t("apps.tabs_overview")}
        size="sm"
        variant={current === undefined ? "primary" : "secondary"}
        href={`/apps/${appKey}`}
      />
      {pages.map((page) => (
        <Button
          key={page.id}
          label={
            page.target === "settings.app"
              ? t("apps.tabs_settings_page", { title: page.title })
              : page.title
          }
          size="sm"
          variant={page.id === current ? "primary" : "secondary"}
          href={`/apps/${appKey}/pages/${page.id}`}
        />
      ))}
    </HStack>
  );
}
