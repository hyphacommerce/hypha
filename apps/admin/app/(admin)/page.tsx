import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";

import { requireStaff } from "@/lib/hypha";

/** The signed-in home: who you are and what you may do. */
export default async function HomePage() {
  const me = await requireStaff();

  return (
    <Section padding={6}>
      <VStack gap={6}>
        <VStack gap={1}>
          <Text type="display-1" as="h1">
            Welcome, {me.user.name}
          </Text>
          <Text type="body" color="secondary">
            Products, orders and store settings are in the menu; customers and markets come next.
          </Text>
        </VStack>

        {!me.mfa_enabled && (
          <Banner
            status="warning"
            title="Two-factor authentication is off"
            description="Turn it on to protect your account; it takes a minute with any authenticator app."
          />
        )}

        <MetadataList title="Your account">
          <MetadataListItem label="Email">{me.user.email}</MetadataListItem>
          <MetadataListItem label="Role">{me.role.name}</MetadataListItem>
          <MetadataListItem label="Two-factor">
            {me.mfa_enabled ? "On" : "Off"}
          </MetadataListItem>
        </MetadataList>

        <VStack gap={2}>
          <Text type="display-3" as="h2">
            What you can do
          </Text>
          <HStack gap={2} wrap="wrap">
            {me.scopes.map((scope) => (
              <Badge key={scope} label={scope} />
            ))}
          </HStack>
        </VStack>
      </VStack>
    </Section>
  );
}
