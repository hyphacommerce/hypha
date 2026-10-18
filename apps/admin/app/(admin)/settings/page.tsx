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
import { visibleSettings } from "@/lib/navigation";

/**
 * Settings: every settings page the user's role can open, grouped, so the side nav
 * needs only one entry.
 */
export default async function SettingsPage() {
  const me = await requireStaff();
  const groups = visibleSettings(me.scopes);
  return (
    <Section padding={6}>
      <VStack gap={6} maxWidth={960}>
        <PageHeader
          title="Settings"
          description="How the store runs, who works on it, and what connects to it."
        />
        {groups.length === 0 && (
          <Banner
            status="info"
            title="Your role has no settings to open"
            description="Ask the store owner if you need access. Your own account is always under Your account."
            endContent={<Button label="Your account" variant="secondary" href="/account" />}
          />
        )}
        {groups.map((group) => (
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
