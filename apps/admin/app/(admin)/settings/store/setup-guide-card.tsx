"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { useT } from "@/lib/use-t";

import { reopenSetup } from "../../setup/actions";

/** Shows the put-away setup guide again. */
export function SetupGuideCard() {
  const t = useT<"settings">();
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
              {t("settings.setup_guide_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("settings.setup_guide_description")}
            </Text>
          </VStack>
          <Button
            label={t("settings.setup_guide_show")}
            variant="secondary"
            isLoading={isPending}
            onClick={reopen}
          />
        </HStack>
        {error && <Banner status="error" title={error} />}
      </VStack>
    </Card>
  );
}
