import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";

import type { PageExtension } from "@/lib/app-extensions";

/** An app's own pages (`apps.page`, `settings.app`), as links between them. */
export function AppPageTabs({
  appKey,
  pages,
  current,
}: {
  appKey: string;
  pages: PageExtension[];
  /** The page shown, or none on the app's overview. */
  current?: string;
}) {
  if (pages.length === 0) return null;
  return (
    <HStack gap={2} wrap="wrap">
      <Button
        label="Overview"
        size="sm"
        variant={current === undefined ? "primary" : "secondary"}
        href={`/apps/${appKey}`}
      />
      {pages.map((page) => (
        <Button
          key={page.id}
          label={page.target === "settings.app" ? `${page.title} (settings)` : page.title}
          size="sm"
          variant={page.id === current ? "primary" : "secondary"}
          href={`/apps/${appKey}/pages/${page.id}`}
        />
      ))}
    </HStack>
  );
}
