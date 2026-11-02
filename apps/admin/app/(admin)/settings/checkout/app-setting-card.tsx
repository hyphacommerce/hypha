"use client";

// An app checkout settings name: the one verifying shipping addresses
// (`address.verify`: asked when a cart's address is set and when staff change an
// order's; it only warns and suggests, a checkout rule on the verdict makes it block), or
// the one suggesting addresses as people type (`address.suggest` and `address.resolve`).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import { type AppSetting, setCheckoutApp } from "./actions";

/** "None" in the picker. */
const NONE = "none";

/** What each setting's card says. */
const TEXT: Record<
  AppSetting,
  { title: string; description: string; picker: string; none: string; empty: string }
> = {
  verification_app: {
    title: "Address verification",
    description:
      "An app that checks whether mail reaches a shipping address and suggests a corrected one. Shoppers see a warning and the suggestion; add a rule on the verdict to stop checkout.",
    picker: "Verification app",
    none: "None: addresses aren't verified",
    empty:
      "No installed app verifies addresses. Install one whose manifest declares address.verify.",
  },
  autocomplete_app: {
    title: "Address autocomplete",
    description:
      "An app that suggests addresses as people type the first line, in the storefront's checkout and account and in the admin's address forms. Picking one fills the form.",
    picker: "Autocomplete app",
    none: "None: addresses are typed",
    empty:
      "No installed app suggests addresses. Install one whose manifest declares address.suggest and address.resolve.",
  },
};

export function AppSettingCard({
  setting,
  current,
  apps,
  canWrite,
}: {
  setting: AppSetting;
  /** The app named now, if any. */
  current: string | null;
  /** The installed apps that answer the setting's points: key and name. */
  apps: { value: string; label: string }[];
  canWrite: boolean;
}) {
  const router = useRouter();
  const [chosen, setChosen] = useState(current ?? NONE);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  // The app named stays a choice even when it no longer answers, so it can be seen.
  const text = TEXT[setting];
  const options = [
    { value: NONE, label: text.none },
    ...apps,
    ...(current && !apps.some((a) => a.value === current)
      ? [{ value: current, label: `${current} (no longer answers)` }]
      : []),
  ];
  const save = () =>
    startTransition(async () => {
      const result = await setCheckoutApp(setting, chosen === NONE ? null : chosen);
      setError(result.error);
      if (!result.error) router.refresh();
    });

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {text.title}
          </Text>
          <Text color="secondary">{text.description}</Text>
        </VStack>
        {error && <Banner status="error" title={error} />}
        {apps.length === 0 && !current ? (
          <Text color="secondary">{text.empty}</Text>
        ) : (
          <HStack gap={2} vAlign="end" wrap="wrap">
            <Selector
              label={text.picker}
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
