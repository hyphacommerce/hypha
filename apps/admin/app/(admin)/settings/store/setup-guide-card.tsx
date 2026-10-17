"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { reopenSetup } from "../../setup/actions";

/** Shows the put-away setup guide again (design note 0024, AU11). */
export function SetupGuideCard() {
  const router = useRouter();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const reopen = () =>
    startTransition(async () => {
      const result = await reopenSetup();
      if (result.error) return setError(result.error);
      router.push("/setup");
    });
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={4} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Setup guide
            </Text>
            <Text type="supporting" color="secondary">
              Finished or hidden. Show it again in the menu and on the home page, from the
              step it was left on.
            </Text>
          </VStack>
          <Button label="Show the setup guide" variant="secondary" isLoading={isPending} onClick={reopen} />
        </HStack>
        {error && <Banner status="error" title={error} />}
      </VStack>
    </Card>
  );
}
