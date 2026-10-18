import { Card } from "@astryxdesign/core/Card";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_GROUPS } from "@/lib/navigation";

/** Settings: every settings page, grouped, so the side nav needs only one entry. */
export default function SettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={6} maxWidth={960}>
        <PageHeader
          title="Settings"
          description="How the store runs, who works on it, and what connects to it."
        />
        {SETTINGS_GROUPS.map((group) => (
          <Card key={group.title}>
            <VStack gap={4}>
              <VStack gap={1}>
                <Text type="display-3" as="h2">
                  {group.title}
                </Text>
                <Text type="supporting" color="secondary">
                  {group.description}
                </Text>
              </VStack>
              <Grid columns={{ minWidth: 240, repeat: "fill" }} gap={3}>
                {group.pages.map(({ label, href, icon: Icon, description }) => (
                  <ClickableCard key={href} label={label} href={href} variant="muted" padding={4}>
                    <HStack gap={3} vAlign="start">
                      <Text color="secondary">
                        <Icon width={20} aria-hidden />
                      </Text>
                      <VStack gap={1}>
                        <Text weight="medium">{label}</Text>
                        <Text type="supporting" color="secondary">
                          {description}
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
