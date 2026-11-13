import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Section } from "@astryxdesign/core/Section";

import { loadStore } from "@/lib/api";
import { getT } from "@/lib/i18n";
import { pageLanguage } from "@/lib/site";

/** The store's language, or English when the API can't be read (this page is
 * prerendered at build time, without one). */
async function language(): Promise<string> {
  try {
    return pageLanguage(undefined, await loadStore());
  } catch {
    return "en";
  }
}

export default async function NotFound() {
  const t = await getT(await language());
  return (
    <Section padding={10}>
      <EmptyState
        headingLevel={1}
        title={t("common.not_found_title")}
        description={t("common.not_found_description")}
        actions={<Button label={t("common.back_to_shop")} href="/" variant="primary" />}
      />
    </Section>
  );
}
