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

import { loadSetup, requireStaff } from "@/lib/hypha";

import { type SetupStep, setupSteps } from "./setup-checklist";

/** The signed-in home: what's left to set up, who you are and what you may do. */
export default async function HomePage() {
  const [me, steps, setup] = await Promise.all([requireStaff(), setupSteps(), loadSetup()]);

  return (
    <Section padding={6}>
      <VStack gap={6}>
        <VStack gap={1}>
          <Text type="display-1" as="h1">
            Welcome, {me.user.name}
          </Text>
          <Text type="body" color="secondary">
            What needs doing in your store, and your account.
          </Text>
        </VStack>

        {!me.mfa_enabled && (
          <Banner
            status="warning"
            title="Two-factor authentication is off"
            description="Turn it on to protect your account; it takes a minute with any authenticator app."
          />
        )}

        {/* Only for staff who can see methods: without payments:read the checklist's
            step reads as not done whatever the store has. */}
        {(me.scopes.includes("payments:read") || me.scopes.includes("payments:write")) &&
          steps.some((step) => step.key === "payment" && !step.isDone) && (
            <Banner
              status="warning"
              title="Checkout is refused until a payment method is enabled"
              description="Shoppers can't place orders with something to pay."
              endContent={
                <Button label="Set up payments" variant="secondary" href="/settings/payments" />
              }
            />
          )}

        {setup && !setup.isFinished && <SetupChecklist steps={steps} />}

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
          <Button label="Continue the setup guide" variant="primary" href="/setup" />
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
