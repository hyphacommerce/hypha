"use client";

// The app that verifies shipping addresses (`address.verify`): asked when a cart's
// address is set and when staff change an order's. Its answer only warns and suggests;
// a checkout rule on the verdict makes it block.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { setVerificationApp } from "./actions";

/** "None" in the picker. */
const NONE = "none";

export function VerificationCard({
  current,
  apps,
  canWrite,
}: {
  /** The app named now, if any. */
  current: string | null;
  /** The installed apps that answer `address.verify`: key and name. */
  apps: { value: string; label: string }[];
  canWrite: boolean;
}) {
  const router = useRouter();
  const [chosen, setChosen] = useState(current ?? NONE);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // The app named stays a choice even when it no longer answers, so it can be seen.
  const options = [
    { value: NONE, label: "None: addresses aren't verified" },
    ...apps,
    ...(current && !apps.some((a) => a.value === current)
      ? [{ value: current, label: `${current} (no longer answers)` }]
      : []),
  ];
  const save = () =>
    startTransition(async () => {
      const result = await setVerificationApp(chosen === NONE ? null : chosen);
      setError(result.error);
      if (!result.error) router.refresh();
    });

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Address verification
          </Text>
          <Text color="secondary">
            An app that checks whether mail reaches a shipping address and suggests a corrected one.
            Shoppers see a warning and the suggestion; add a rule on the verdict to stop checkout.
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {apps.length === 0 && !current ? (
          <Text color="secondary">
            No installed app verifies addresses. Install one whose manifest declares address.verify.
          </Text>
        ) : (
          <HStack gap={2} vAlign="end" wrap="wrap">
            <Selector
              label="Verification app"
              options={options}
              value={chosen}
              onChange={setChosen}
              isDisabled={!canWrite}
            />
            {canWrite && (
              <Button
                label="Save"
                variant="primary"
                isLoading={isPending}
                isDisabled={chosen === (current ?? NONE)}
                onClick={save}
              />
            )}
          </HStack>
        )}
      </VStack>
    </Card>
  );
}
