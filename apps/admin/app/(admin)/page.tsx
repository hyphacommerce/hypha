import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { CheckCircleIcon as CheckCircleOutline } from "@heroicons/react/24/outline";
import { CheckCircleIcon } from "@heroicons/react/24/solid";

import { requireStaff } from "@/lib/hypha";

import { type SetupStep, setupSteps } from "./setup-checklist";

/** The signed-in home: what's left to set up, who you are and what you may do. */
export default async function HomePage() {
  const [me, steps] = await Promise.all([requireStaff(), setupSteps()]);

  return (
    <Section padding={6}>
      <VStack gap={6}>
        <VStack gap={1}>
          <Text type="display-1" as="h1">
            Welcome, {me.user.name}
          </Text>
          <Text type="body" color="secondary">
            Products, orders, markets and store settings are in the menu; customers come next.
          </Text>
        </VStack>

        {!me.mfa_enabled && (
          <Banner
            status="warning"
            title="Two-factor authentication is off"
            description="Turn it on to protect your account; it takes a minute with any authenticator app."
          />
        )}

        {steps.some((step) => !step.isDone) && <SetupChecklist steps={steps} />}

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

/** What a storefront needs before it can sell, ticked as the data shows it done. */
function SetupChecklist({ steps }: { steps: SetupStep[] }) {
  const done = steps.filter((step) => step.isDone).length;
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={3} hAlign="between" vAlign="start" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Get ready to sell
            </Text>
            <Text type="supporting" color="secondary">
              {done} of {steps.length} done. Each step opens the screen it's done on.
            </Text>
          </VStack>
          <Button label="Set up your store" variant="primary" href="/setup" />
        </HStack>
        <VStack gap={3}>
          {steps.map((step) => (
            <HStack key={step.label} gap={3} vAlign="start">
              <Text color={step.isDone ? undefined : "secondary"}>
                {step.isDone ? (
                  <CheckCircleIcon width={20} aria-label="Done" />
                ) : (
                  <CheckCircleOutline width={20} aria-label="Not done yet" />
                )}
              </Text>
              <VStack gap={0}>
                {step.isDone ? (
                  <Text color="secondary">{step.label}</Text>
                ) : (
                  <Link href={step.href} weight="medium">
                    {step.label}
                  </Link>
                )}
                <Text type="supporting" color="secondary">
                  {step.description}
                </Text>
              </VStack>
            </HStack>
          ))}
        </VStack>
      </VStack>
    </Card>
  );
}
