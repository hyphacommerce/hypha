import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { getT } from "@/lib/i18n";

/**
 * What a page shows when its API reads need a scope the user's role lacks
 * (`forbidden()`, from `loaded` in lib/hypha.ts): a notice inside the frame instead of
 * the error page, so the rest of the admin stays a click away.
 */
export default async function Forbidden() {
  const t = await getT();
  return (
    <Section padding={6}>
      <Banner
        status="info"
        title={t("shell.forbidden_title")}
        description={t("shell.forbidden_description")}
        endContent={
          <HStack gap={2}>
            <Button label={t("common.home")} variant="secondary" href="/" />
            <Button label={t("common.your_account")} variant="secondary" href="/account" />
          </HStack>
        }
      />
    </Section>
  );
}
