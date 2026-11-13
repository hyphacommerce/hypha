import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { requireStaff } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { visibleSettings } from "@/lib/navigation";

/**
 * Settings: every settings page the user's role can open, grouped, so the side nav
 * needs only one entry.
 */
export default async function SettingsPage() {
  const [me, t] = await Promise.all([requireStaff(), getT()]);
  const groups = visibleSettings(me.scopes);
  return (
    <Section padding={6}>
      <VStack gap={6} maxWidth={960}>
        <PageHeader title={t("common.settings")} description={t("settings.index_description")} />
        {groups.length === 0 && (
          <Banner
            status="info"
            title={t("settings.index_empty_title")}
            description={t("settings.index_empty_description")}
            endContent={
              <Button label={t("common.your_account")} variant="secondary" href="/account" />
            }
          />
        )}
        {groups.map((group) => (
          <Card key={group.title}>
            <VStack gap={4}>
              <VStack gap={1}>
                <Text type="display-3" as="h2">
                  {t(group.title)}
                </Text>
                <Text type="supporting" color="secondary">
                  {t(group.description)}
                </Text>
              </VStack>
              <Grid columns={{ minWidth: 240, repeat: "fill" }} gap={3}>
                {group.pages.map(({ label, href, icon: Icon, description }) => (
                  <ClickableCard
                    key={href}
                    label={t(label)}
                    href={href}
                    variant="muted"
                    padding={4}
                  >
                    <HStack gap={3} vAlign="start">
                      <Text color="secondary">
                        <Icon width={20} aria-hidden />
                      </Text>
                      <VStack gap={1}>
                        <Text weight="medium">{t(label)}</Text>
                        <Text type="supporting" color="secondary">
                          {t(description)}
                        </Text>
                      </VStack>
                    </HStack>
                  </ClickableCard>
                ))}
              </Grid>
            </VStack>
          </Card>
        ))}
      </VStack>
    </Section>
  );
}
